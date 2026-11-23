//! The overlay: hooks of `IDXGISwapChain::Present` / `Present1` (and of
//! `ID3D12CommandQueue::ExecuteCommandLists`, to learn which queue the presenter uses) and a small
//! D3D12 renderer that draws a list of quads — rectangles and anti-aliased Inter glyphs from an
//! atlas in an upload buffer — on the back buffer right before it is presented. Works with any
//! D3D12 presenter on Windows.

use crate::hook;
use scripthook_sdk::font;
use shk_sys::*;
use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

// ---------------------------------------------------------------------------------------------
// Draw list

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Vertex {
    pub x: f32,
    pub y: f32,
    pub u: f32,
    pub v: f32,
    /// R | G<<8 | B<<16 | A<<24
    pub rgba: u32,
}

/// 0xAARRGGBB → vertex colour.
fn rgba(argb: u32) -> u32 {
    let (a, r, g, b) = (argb >> 24, (argb >> 16) & 0xFF, (argb >> 8) & 0xFF, argb & 0xFF);
    r | g << 8 | b << 16 | a << 24
}

#[derive(Default)]
pub struct DrawList {
    pub verts: Vec<Vertex>,
    clip: Option<(f32, f32, f32, f32)>,
    pub width: u32,
    pub height: u32,
}

impl DrawList {
    pub fn new(width: u32, height: u32) -> DrawList {
        DrawList { verts: Vec::new(), clip: None, width, height }
    }

    /// A textured quad, clipped (uv follows the clip).
    fn quad(&mut self, mut x0: f32, mut y0: f32, mut x1: f32, mut y1: f32, mut u0: f32, mut v0: f32, mut u1: f32, mut v1: f32, c: u32) {
        let (cx0, cy0, cx1, cy1) = self.clip.unwrap_or((0.0, 0.0, self.width as f32, self.height as f32));
        if x1 <= cx0 || y1 <= cy0 || x0 >= cx1 || y0 >= cy1 || x1 <= x0 || y1 <= y0 {
            return;
        }
        let (du, dv) = ((u1 - u0) / (x1 - x0), (v1 - v0) / (y1 - y0));
        if x0 < cx0 {
            u0 += (cx0 - x0) * du;
            x0 = cx0;
        }
        if y0 < cy0 {
            v0 += (cy0 - y0) * dv;
            y0 = cy0;
        }
        if x1 > cx1 {
            u1 -= (x1 - cx1) * du;
            x1 = cx1;
        }
        if y1 > cy1 {
            v1 -= (y1 - cy1) * dv;
            y1 = cy1;
        }
        let v = |x, y, u, vv| Vertex { x, y, u, v: vv, rgba: c };
        self.verts.extend_from_slice(&[v(x0, y0, u0, v0), v(x1, y0, u1, v0), v(x0, y1, u0, v1), v(x0, y1, u0, v1), v(x1, y0, u1, v0), v(x1, y1, u1, v1)]);
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, argb: u32) {
        let s = font::atlas().solid;
        let (u, v) = (s.0 as f32 + 0.5, s.1 as f32 + 0.5);
        self.quad(x.round(), y.round(), (x + w).round(), (y + h).round(), u, v, u, v, rgba(argb));
    }

    /// Text with its cap line at y; returns the width.
    pub fn text(&mut self, x: f32, y: f32, scale: f32, argb: u32, s: &str) -> f32 {
        let at = font::atlas();
        let face = at.face(font::px_for_scale(scale));
        let c = rgba(argb);
        let mut pen = x;
        for ch in s.chars() {
            let g = at.glyph(face, ch);
            if g.w > 0 {
                let gx = (pen + g.left).round();
                let gy = (y + g.top).round();
                self.quad(gx, gy, gx + g.w as f32, gy + g.h as f32, g.x as f32, g.y as f32, (g.x + g.w) as f32, (g.y + g.h) as f32, c);
            }
            pen += g.advance;
        }
        pen - x
    }

    pub fn set_clip(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.clip = if w <= 0.0 || h <= 0.0 { None } else { Some((x, y, x + w, y + h)) };
    }
}

// ---------------------------------------------------------------------------------------------
// What draws: the host side calls this every present (set by the core)

pub type FrameHook = fn(&mut DrawList, u32, u32);
static FRAME: Mutex<Option<FrameHook>> = Mutex::new(None);

pub fn set_frame_hook(f: FrameHook) {
    *FRAME.lock().unwrap_or_else(|e| e.into_inner()) = Some(f);
}

/// A capture of the next presented image with the overlay (PNG), for tests and screenshots.
static CAPTURE: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);
pub static PRESENTS: AtomicU64 = AtomicU64::new(0);
pub static DRAWN: AtomicU64 = AtomicU64::new(0);

pub fn capture_next(path: std::path::PathBuf) {
    *CAPTURE.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
}

// ---------------------------------------------------------------------------------------------
// Hooks

thread_local! {
    static LAST_QUEUE: Cell<usize> = const { Cell::new(0) };
}
static ANY_QUEUE: AtomicUsize = AtomicUsize::new(0);
static ECL_ORIG: AtomicUsize = AtomicUsize::new(0);
static PRESENT_ORIG: AtomicUsize = AtomicUsize::new(0);
static PRESENT1_ORIG: AtomicUsize = AtomicUsize::new(0);

unsafe fn queue_type(q: Unk) -> u32 {
    let mut d = D3D12_COMMAND_QUEUE_DESC::default();
    vfn::<unsafe extern "system" fn(Unk, *mut D3D12_COMMAND_QUEUE_DESC) -> *mut D3D12_COMMAND_QUEUE_DESC>(q, 18)(q, &mut d);
    d.Type
}

unsafe extern "system" fn ecl_detour(q: Unk, n: u32, lists: *const Unk) {
    if LAST_QUEUE.with(|c| c.get()) != q as usize && queue_type(q) == D3D12_COMMAND_LIST_TYPE_DIRECT {
        LAST_QUEUE.with(|c| c.set(q as usize));
        ANY_QUEUE.store(q as usize, Ordering::Relaxed);
    }
    let o: unsafe extern "system" fn(Unk, u32, *const Unk) = std::mem::transmute(ECL_ORIG.load(Ordering::Acquire));
    o(q, n, lists)
}

unsafe extern "system" fn present_detour(sc: Unk, sync: u32, flags: u32) -> HRESULT {
    on_present(sc, flags);
    let o: unsafe extern "system" fn(Unk, u32, u32) -> HRESULT = std::mem::transmute(PRESENT_ORIG.load(Ordering::Acquire));
    o(sc, sync, flags)
}

unsafe extern "system" fn present1_detour(sc: Unk, sync: u32, flags: u32, params: *const c_void) -> HRESULT {
    on_present(sc, flags);
    let o: unsafe extern "system" fn(Unk, u32, u32, *const c_void) -> HRESULT = std::mem::transmute(PRESENT1_ORIG.load(Ordering::Acquire));
    o(sc, sync, flags, params)
}

unsafe extern "system" fn dummy_wndproc(h: HWND, m: u32, w: usize, l: isize) -> isize {
    DefWindowProcW(h, m, w, l)
}

/// Finds the D3D12 / DXGI functions through a throw-away device and swap chain and hooks them.
pub fn install() -> Result<String, String> {
    // SAFETY: COM calls on objects created here; released at the end.
    unsafe {
        let mut dev: Unk = std::ptr::null_mut();
        if D3D12CreateDevice(std::ptr::null_mut(), 0xb000, &IID_ID3D12Device, &mut dev) < 0 || dev.is_null() {
            return Err("D3D12CreateDevice failed (no D3D12 device for the probe)".into());
        }
        let qd = D3D12_COMMAND_QUEUE_DESC::default();
        let mut q: Unk = std::ptr::null_mut();
        vfn::<unsafe extern "system" fn(Unk, *const D3D12_COMMAND_QUEUE_DESC, *const GUID, *mut Unk) -> HRESULT>(dev, 8)(dev, &qd, &IID_ID3D12CommandQueue, &mut q);
        if q.is_null() {
            release(dev);
            return Err("CreateCommandQueue failed".into());
        }
        let mut f: Unk = std::ptr::null_mut();
        CreateDXGIFactory2(0, &IID_IDXGIFactory4, &mut f);
        let cls = wide("shk-overlay-probe");
        let wc = WNDCLASSEXW { cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32, style: 0, lpfnWndProc: dummy_wndproc, cbClsExtra: 0, cbWndExtra: 0, hInstance: GetModuleHandleW(std::ptr::null()), hIcon: std::ptr::null_mut(), hCursor: std::ptr::null_mut(), hbrBackground: std::ptr::null_mut(), lpszMenuName: std::ptr::null(), lpszClassName: cls.as_ptr(), hIconSm: std::ptr::null_mut() };
        RegisterClassExW(&wc);
        let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW, cls.as_ptr(), cls.as_ptr(), WS_POPUP, 0, 0, 64, 64, std::ptr::null_mut(), std::ptr::null_mut(), wc.hInstance, std::ptr::null_mut());
        let mut sc: Unk = std::ptr::null_mut();
        if !f.is_null() && !hwnd.is_null() {
            let d = DXGI_SWAP_CHAIN_DESC1 { Width: 64, Height: 64, Format: DXGI_FORMAT_R8G8B8A8_UNORM, SampleCount: 1, BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT, BufferCount: 2, SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD, ..Default::default() };
            vfn::<unsafe extern "system" fn(Unk, Unk, HWND, *const DXGI_SWAP_CHAIN_DESC1, *const c_void, Unk, *mut Unk) -> HRESULT>(f, 15)(f, q, hwnd, &d, std::ptr::null(), std::ptr::null_mut(), &mut sc);
        }
        let ecl = vslot(q, 10);
        let (present, present1) = if sc.is_null() { (0, 0) } else { (vslot(sc, 8), vslot(sc, 22)) };
        release(sc);
        if !hwnd.is_null() {
            DestroyWindow(hwnd);
        }
        release(f);
        release(q);
        release(dev);
        if present == 0 {
            return Err("could not create a probe swap chain".into());
        }
        let (_, t) = hook::install(ecl, ecl_detour as *const () as usize, 0).map_err(|e| format!("ExecuteCommandLists: {e}"))?;
        ECL_ORIG.store(t, Ordering::Release);
        let (_, t) = hook::install(present, present_detour as *const () as usize, 0).map_err(|e| format!("Present: {e}"))?;
        PRESENT_ORIG.store(t, Ordering::Release);
        let p1 = match hook::install(present1, present1_detour as *const () as usize, 0) {
            Ok((_, t)) => {
                PRESENT1_ORIG.store(t, Ordering::Release);
                "Present1 too"
            }
            Err(_) => "not Present1",
        };
        Ok(format!("hooked IDXGISwapChain::Present at {present:#x} ({p1}) and ExecuteCommandLists at {ecl:#x}"))
    }
}

// ---------------------------------------------------------------------------------------------
// Renderer

const SHADER: &str = r#"
cbuffer C : register(b0) { float2 screen; float atlasW; float pad; };
ByteAddressBuffer atlas : register(t0);
struct VI { float2 pos : POSITION; float2 uv : TEXCOORD; float4 col : COLOR; };
struct VO { float4 pos : SV_Position; float2 uv : TEXCOORD; float4 col : COLOR; };
VO vs(VI i) { VO o; o.pos = float4(i.pos.x / screen.x * 2 - 1, 1 - i.pos.y / screen.y * 2, 0, 1); o.uv = i.uv; o.col = i.col; return o; }
float4 ps(VO i) : SV_Target {
    uint2 t = uint2(i.uv);
    uint idx = t.y * (uint)atlasW + t.x;
    uint w = atlas.Load(idx & ~3u);
    float c = ((w >> ((idx & 3u) * 8u)) & 0xFFu) / 255.0;
    return float4(i.col.rgb, i.col.a * c);
}
"#;

const SLOTS: usize = 4;
const VB_BYTES: usize = 1 << 20;

#[allow(dead_code)] // COM objects kept alive with the renderer
struct Renderer {
    device: Unk,
    queue: Unk,
    root: Unk,
    psos: Vec<(u32, Unk)>,
    rtv_heap: Unk,
    rtv_start: usize,
    rtv_step: usize,
    allocs: [Unk; SLOTS],
    list: Unk,
    fence: Unk,
    fence_val: u64,
    slot_val: [u64; SLOTS],
    atlas: Unk,
    atlas_va: u64,
    vb: Unk,
    vb_ptr: *mut u8,
    vb_va: u64,
    vs: Vec<u8>,
    ps: Vec<u8>,
    frame: usize,
}

// SAFETY: used under the RENDER lock on the presenting thread.
unsafe impl Send for Renderer {}

static RENDER: Mutex<Option<Renderer>> = Mutex::new(None);
static FAILED: Mutex<Option<String>> = Mutex::new(None);

unsafe fn upload_buffer(dev: Unk, size: usize) -> Result<(Unk, *mut u8, u64), String> {
    let hp = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_UPLOAD, ..Default::default() };
    let rd = D3D12_RESOURCE_DESC::buffer(size as u64);
    let mut r: Unk = std::ptr::null_mut();
    let hr = vfn::<unsafe extern "system" fn(Unk, *const D3D12_HEAP_PROPERTIES, u32, *const D3D12_RESOURCE_DESC, u32, *const c_void, *const GUID, *mut Unk) -> HRESULT>(dev, 27)(dev, &hp, 0, &rd, D3D12_RESOURCE_STATE_GENERIC_READ, std::ptr::null(), &IID_ID3D12Resource, &mut r);
    if hr < 0 || r.is_null() {
        return Err(format!("CreateCommittedResource (upload, {size} bytes): {hr:#x}"));
    }
    let mut p: *mut c_void = std::ptr::null_mut();
    let none = [0usize, 0usize];
    vfn::<unsafe extern "system" fn(Unk, u32, *const usize, *mut *mut c_void) -> HRESULT>(r, 8)(r, 0, none.as_ptr(), &mut p);
    let va = vfn::<unsafe extern "system" fn(Unk) -> u64>(r, 11)(r);
    Ok((r, p as *mut u8, va))
}

impl Renderer {
    unsafe fn new(queue: Unk) -> Result<Renderer, String> {
        let mut dev: Unk = std::ptr::null_mut();
        vfn::<unsafe extern "system" fn(Unk, *const GUID, *mut Unk) -> HRESULT>(queue, 7)(queue, &IID_ID3D12Device, &mut dev);
        if dev.is_null() {
            return Err("the queue has no device".into());
        }
        let vs = d3d_compile(SHADER, "vs", "vs_5_0")?;
        let ps = d3d_compile(SHADER, "ps", "ps_5_0")?;
        // root signature: 4 constants (b0) + a root SRV (t0)
        let params = [
            D3D12_ROOT_PARAMETER { ParameterType: 1, _pad0: 0, a: 0, b: 0, c: 4, _pad1: 0, ShaderVisibility: 0, _pad2: 0 },
            D3D12_ROOT_PARAMETER { ParameterType: 3, _pad0: 0, a: 0, b: 0, c: 0, _pad1: 0, ShaderVisibility: 5, _pad2: 0 },
        ];
        let rsd = D3D12_ROOT_SIGNATURE_DESC { NumParameters: 2, pParameters: params.as_ptr(), NumStaticSamplers: 0, pStaticSamplers: std::ptr::null(), Flags: 1 };
        let (mut blob, mut err): (Unk, Unk) = (std::ptr::null_mut(), std::ptr::null_mut());
        if D3D12SerializeRootSignature(&rsd, 1, &mut blob, &mut err) < 0 || blob.is_null() {
            let m = if err.is_null() { String::new() } else { String::from_utf8_lossy(&blob_bytes(err)).into_owned() };
            release(err);
            return Err(format!("root signature: {m}"));
        }
        let b = blob_bytes(blob);
        release(blob);
        let mut root: Unk = std::ptr::null_mut();
        vfn::<unsafe extern "system" fn(Unk, u32, *const c_void, usize, *const GUID, *mut Unk) -> HRESULT>(dev, 16)(dev, 0, b.as_ptr() as *const c_void, b.len(), &IID_ID3D12RootSignature, &mut root);
        if root.is_null() {
            return Err("CreateRootSignature failed".into());
        }
        let hd = D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV, NumDescriptors: 16, Flags: 0, NodeMask: 0 };
        let mut heap: Unk = std::ptr::null_mut();
        vfn::<unsafe extern "system" fn(Unk, *const D3D12_DESCRIPTOR_HEAP_DESC, *const GUID, *mut Unk) -> HRESULT>(dev, 14)(dev, &hd, &IID_ID3D12DescriptorHeap, &mut heap);
        if heap.is_null() {
            return Err("CreateDescriptorHeap failed".into());
        }
        let mut start = 0usize;
        vfn::<unsafe extern "system" fn(Unk, *mut usize) -> *mut usize>(heap, 9)(heap, &mut start);
        let step = vfn::<unsafe extern "system" fn(Unk, u32) -> u32>(dev, 15)(dev, D3D12_DESCRIPTOR_HEAP_TYPE_RTV) as usize;
        let mut allocs = [std::ptr::null_mut(); SLOTS];
        for a in allocs.iter_mut() {
            vfn::<unsafe extern "system" fn(Unk, u32, *const GUID, *mut Unk) -> HRESULT>(dev, 9)(dev, 0, &IID_ID3D12CommandAllocator, a);
            if a.is_null() {
                return Err("CreateCommandAllocator failed".into());
            }
        }
        let mut list: Unk = std::ptr::null_mut();
        vfn::<unsafe extern "system" fn(Unk, u32, u32, Unk, Unk, *const GUID, *mut Unk) -> HRESULT>(dev, 12)(dev, 0, 0, allocs[0], std::ptr::null_mut(), &IID_ID3D12GraphicsCommandList, &mut list);
        if list.is_null() {
            return Err("CreateCommandList failed".into());
        }
        vfn::<unsafe extern "system" fn(Unk) -> HRESULT>(list, 9)(list);
        let mut fence: Unk = std::ptr::null_mut();
        vfn::<unsafe extern "system" fn(Unk, u64, u32, *const GUID, *mut Unk) -> HRESULT>(dev, 36)(dev, 0, 0, &IID_ID3D12Fence, &mut fence);
        if fence.is_null() {
            return Err("CreateFence failed".into());
        }
        let at = font::atlas();
        let (atlas, ap, atlas_va) = upload_buffer(dev, at.pixels.len().next_multiple_of(4))?;
        std::ptr::copy_nonoverlapping(at.pixels.as_ptr(), ap, at.pixels.len());
        let (vb, vb_ptr, vb_va) = upload_buffer(dev, VB_BYTES * SLOTS)?;
        Ok(Renderer { device: dev, queue, root, psos: Vec::new(), rtv_heap: heap, rtv_start: start, rtv_step: step, allocs, list, fence, fence_val: 0, slot_val: [0; SLOTS], atlas, atlas_va, vb, vb_ptr, vb_va, vs, ps, frame: 0 })
    }

    unsafe fn pso(&mut self, format: u32) -> Result<Unk, String> {
        if let Some(p) = self.psos.iter().find(|p| p.0 == format) {
            return Ok(p.1);
        }
        let il = [
            D3D12_INPUT_ELEMENT_DESC { SemanticName: c"POSITION".as_ptr() as *const u8, SemanticIndex: 0, Format: DXGI_FORMAT_R32G32_FLOAT, InputSlot: 0, AlignedByteOffset: 0, InputSlotClass: 0, InstanceDataStepRate: 0 },
            D3D12_INPUT_ELEMENT_DESC { SemanticName: c"TEXCOORD".as_ptr() as *const u8, SemanticIndex: 0, Format: DXGI_FORMAT_R32G32_FLOAT, InputSlot: 0, AlignedByteOffset: 8, InputSlotClass: 0, InstanceDataStepRate: 0 },
            D3D12_INPUT_ELEMENT_DESC { SemanticName: c"COLOR".as_ptr() as *const u8, SemanticIndex: 0, Format: DXGI_FORMAT_R8G8B8A8_UNORM, InputSlot: 0, AlignedByteOffset: 16, InputSlotClass: 0, InstanceDataStepRate: 0 },
        ];
        let none = D3D12_SHADER_BYTECODE { ptr: std::ptr::null(), len: 0 };
        let mut d: D3D12_GRAPHICS_PIPELINE_STATE_DESC = std::mem::zeroed();
        d.pRootSignature = self.root;
        d.VS = D3D12_SHADER_BYTECODE { ptr: self.vs.as_ptr() as *const c_void, len: self.vs.len() };
        d.PS = D3D12_SHADER_BYTECODE { ptr: self.ps.as_ptr() as *const c_void, len: self.ps.len() };
        d.DS = none;
        d.HS = none;
        d.GS = none;
        d.RenderTarget[0] = D3D12_RENDER_TARGET_BLEND_DESC { BlendEnable: 1, LogicOpEnable: 0, SrcBlend: 5, DestBlend: 6, BlendOp: 1, SrcBlendAlpha: 2, DestBlendAlpha: 6, BlendOpAlpha: 1, LogicOp: 4, RenderTargetWriteMask: 0xF };
        d.SampleMask = u32::MAX;
        d.FillMode = 3;
        d.CullMode = 1;
        d.DepthClipEnable = 1;
        d.DepthFunc = 8;
        d.FrontFace = [1, 1, 1, 8];
        d.BackFace = [1, 1, 1, 8];
        d.pInputElementDescs = il.as_ptr();
        d.NumElements = 3;
        d.PrimitiveTopologyType = 3;
        d.NumRenderTargets = 1;
        d.RTVFormats[0] = format;
        d.SampleCount = 1;
        let mut p: Unk = std::ptr::null_mut();
        let hr = vfn::<unsafe extern "system" fn(Unk, *const D3D12_GRAPHICS_PIPELINE_STATE_DESC, *const GUID, *mut Unk) -> HRESULT>(self.device, 10)(self.device, &d, &IID_ID3D12PipelineState, &mut p);
        if hr < 0 || p.is_null() {
            return Err(format!("CreateGraphicsPipelineState (format {format}): {hr:#x}"));
        }
        self.psos.push((format, p));
        Ok(p)
    }

    unsafe fn wait(&self, v: u64) {
        let f = self.fence;
        let mut spins = 0;
        while vfn::<unsafe extern "system" fn(Unk) -> u64>(f, 8)(f) < v && spins < 20000 {
            Sleep(0);
            spins += 1;
        }
    }

    /// Records and submits the draw list on the back buffer; optionally reads it back.
    unsafe fn draw(&mut self, sc: Unk, dl: &DrawList, capture: Option<&std::path::Path>) -> Result<(), String> {
        let Some(sc3) = query(sc, &IID_IDXGISwapChain3) else { return Err("not a flip-model swap chain".into()) };
        let idx = vfn::<unsafe extern "system" fn(Unk) -> u32>(sc3, 36)(sc3) as usize;
        let mut desc = DXGI_SWAP_CHAIN_DESC1::default();
        vfn::<unsafe extern "system" fn(Unk, *mut DXGI_SWAP_CHAIN_DESC1) -> HRESULT>(sc3, 18)(sc3, &mut desc);
        let mut buf: Unk = std::ptr::null_mut();
        vfn::<unsafe extern "system" fn(Unk, u32, *const GUID, *mut Unk) -> HRESULT>(sc3, 9)(sc3, idx as u32, &IID_ID3D12Resource, &mut buf);
        release(sc3);
        if buf.is_null() {
            return Err("GetBuffer failed".into());
        }
        let pso = match self.pso(desc.Format) {
            Ok(p) => p,
            Err(e) => {
                release(buf);
                return Err(e);
            }
        };
        let slot = self.frame % SLOTS;
        self.frame += 1;
        self.wait(self.slot_val[slot]);
        let rtv = self.rtv_start + (idx % 16) * self.rtv_step;
        vfn::<unsafe extern "system" fn(Unk, Unk, *const c_void, usize)>(self.device, 20)(self.device, buf, std::ptr::null(), rtv);
        let n = dl.verts.len().min(VB_BYTES / std::mem::size_of::<Vertex>());
        let vb_off = slot * VB_BYTES;
        std::ptr::copy_nonoverlapping(dl.verts.as_ptr() as *const u8, self.vb_ptr.add(vb_off), n * std::mem::size_of::<Vertex>());
        let (a, l) = (self.allocs[slot], self.list);
        vfn::<unsafe extern "system" fn(Unk) -> HRESULT>(a, 8)(a);
        vfn::<unsafe extern "system" fn(Unk, Unk, Unk) -> HRESULT>(l, 10)(l, a, pso);
        let b1 = D3D12_RESOURCE_TRANSITION_BARRIER::new(buf, D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_RENDER_TARGET);
        vfn::<unsafe extern "system" fn(Unk, u32, *const D3D12_RESOURCE_TRANSITION_BARRIER)>(l, 26)(l, 1, &b1);
        vfn::<unsafe extern "system" fn(Unk, Unk)>(l, 30)(l, self.root);
        let consts = [(desc.Width as f32).to_bits(), (desc.Height as f32).to_bits(), (font::atlas().width as f32).to_bits(), 0];
        vfn::<unsafe extern "system" fn(Unk, u32, u32, *const u32, u32)>(l, 36)(l, 0, 4, consts.as_ptr(), 0);
        vfn::<unsafe extern "system" fn(Unk, u32, u64)>(l, 40)(l, 1, self.atlas_va);
        let vp = [0.0f32, 0.0, desc.Width as f32, desc.Height as f32, 0.0, 1.0];
        vfn::<unsafe extern "system" fn(Unk, u32, *const f32)>(l, 21)(l, 1, vp.as_ptr());
        let sr = [0i32, 0, desc.Width as i32, desc.Height as i32];
        vfn::<unsafe extern "system" fn(Unk, u32, *const i32)>(l, 22)(l, 1, sr.as_ptr());
        vfn::<unsafe extern "system" fn(Unk, u32)>(l, 20)(l, 4);
        let vbv = D3D12_VERTEX_BUFFER_VIEW { BufferLocation: self.vb_va + vb_off as u64, SizeInBytes: (n * std::mem::size_of::<Vertex>()).max(20) as u32, StrideInBytes: std::mem::size_of::<Vertex>() as u32 };
        vfn::<unsafe extern "system" fn(Unk, u32, u32, *const D3D12_VERTEX_BUFFER_VIEW)>(l, 44)(l, 0, 1, &vbv);
        vfn::<unsafe extern "system" fn(Unk, u32, *const usize, BOOL, *const usize)>(l, 46)(l, 1, &rtv, 0, std::ptr::null());
        if n > 0 {
            vfn::<unsafe extern "system" fn(Unk, u32, u32, u32, u32)>(l, 12)(l, n as u32, 1, 0, 0);
        }
        // optional readback
        let mut rb: Option<(Unk, *mut u8, u32)> = None;
        if capture.is_some() {
            let pitch = (desc.Width * 4).next_multiple_of(256);
            let hp = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_READBACK, ..Default::default() };
            let rd = D3D12_RESOURCE_DESC::buffer(pitch as u64 * desc.Height as u64);
            let mut r: Unk = std::ptr::null_mut();
            vfn::<unsafe extern "system" fn(Unk, *const D3D12_HEAP_PROPERTIES, u32, *const D3D12_RESOURCE_DESC, u32, *const c_void, *const GUID, *mut Unk) -> HRESULT>(self.device, 27)(self.device, &hp, 0, &rd, D3D12_RESOURCE_STATE_COPY_DEST, std::ptr::null(), &IID_ID3D12Resource, &mut r);
            if !r.is_null() {
                let b2 = D3D12_RESOURCE_TRANSITION_BARRIER::new(buf, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COPY_SOURCE);
                vfn::<unsafe extern "system" fn(Unk, u32, *const D3D12_RESOURCE_TRANSITION_BARRIER)>(l, 26)(l, 1, &b2);
                let dst = D3D12_TEXTURE_COPY_LOCATION { pResource: r, Type: 1, _pad: 0, Footprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT { Offset: 0, Format: desc.Format, Width: desc.Width, Height: desc.Height, Depth: 1, RowPitch: pitch } };
                let src = D3D12_TEXTURE_COPY_LOCATION { pResource: buf, Type: 0, _pad: 0, Footprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT { Offset: 0, Format: 0, Width: 0, Height: 0, Depth: 0, RowPitch: 0 } };
                vfn::<unsafe extern "system" fn(Unk, *const D3D12_TEXTURE_COPY_LOCATION, u32, u32, u32, *const D3D12_TEXTURE_COPY_LOCATION, *const c_void)>(l, 16)(l, &dst, 0, 0, 0, &src, std::ptr::null());
                let b3 = D3D12_RESOURCE_TRANSITION_BARRIER::new(buf, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_PRESENT);
                vfn::<unsafe extern "system" fn(Unk, u32, *const D3D12_RESOURCE_TRANSITION_BARRIER)>(l, 26)(l, 1, &b3);
                rb = Some((r, std::ptr::null_mut(), pitch));
            }
        }
        if rb.is_none() {
            let b2 = D3D12_RESOURCE_TRANSITION_BARRIER::new(buf, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_PRESENT);
            vfn::<unsafe extern "system" fn(Unk, u32, *const D3D12_RESOURCE_TRANSITION_BARRIER)>(l, 26)(l, 1, &b2);
        }
        vfn::<unsafe extern "system" fn(Unk) -> HRESULT>(l, 9)(l);
        let lists = [l];
        let ecl: unsafe extern "system" fn(Unk, u32, *const Unk) = match ECL_ORIG.load(Ordering::Acquire) {
            0 => vfn(self.queue, 10),
            t => std::mem::transmute(t),
        };
        ecl(self.queue, 1, lists.as_ptr());
        self.fence_val += 1;
        vfn::<unsafe extern "system" fn(Unk, Unk, u64) -> HRESULT>(self.queue, 14)(self.queue, self.fence, self.fence_val);
        self.slot_val[slot] = self.fence_val;
        release(buf);
        if let (Some((r, _, pitch)), Some(path)) = (rb, capture) {
            self.wait(self.fence_val);
            let mut p: *mut c_void = std::ptr::null_mut();
            vfn::<unsafe extern "system" fn(Unk, u32, *const usize, *mut *mut c_void) -> HRESULT>(r, 8)(r, 0, std::ptr::null(), &mut p);
            if !p.is_null() {
                let (w, h) = (desc.Width as usize, desc.Height as usize);
                let mut px = vec![0u8; w * h * 4];
                for y in 0..h {
                    std::ptr::copy_nonoverlapping((p as *const u8).add(y * pitch as usize), px.as_mut_ptr().add(y * w * 4), w * 4);
                }
                if desc.Format == DXGI_FORMAT_B8G8R8A8_UNORM {
                    for c in px.chunks_mut(4) {
                        c.swap(0, 2);
                    }
                }
                for c in px.chunks_mut(4) {
                    c[3] = 255;
                }
                let _ = std::fs::write(path, scripthook_sdk::png::encode_rgba(w as u32, h as u32, &px));
            }
            release(r);
        }
        Ok(())
    }
}

/// Every present: build the draw list (core, plugins), render it.
unsafe fn on_present(sc: Unk, flags: u32) {
    if flags & 1 != 0 {
        return; // DXGI_PRESENT_TEST
    }
    PRESENTS.fetch_add(1, Ordering::Relaxed);
    let mut desc = DXGI_SWAP_CHAIN_DESC1::default();
    let Some(sc1) = query(sc, &GUID::from_u128(0x790a45f7_0d42_4876_983a_0a55cfe6f4aa)) else { return };
    vfn::<unsafe extern "system" fn(Unk, *mut DXGI_SWAP_CHAIN_DESC1) -> HRESULT>(sc1, 18)(sc1, &mut desc);
    release(sc1);
    let mut dl = DrawList::new(desc.Width, desc.Height);
    let f = *FRAME.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(f) = f {
        f(&mut dl, desc.Width, desc.Height);
    }
    let cap = CAPTURE.lock().unwrap_or_else(|e| e.into_inner()).take();
    if dl.verts.is_empty() && cap.is_none() {
        return;
    }
    let q = match LAST_QUEUE.with(|c| c.get()) {
        0 => ANY_QUEUE.load(Ordering::Relaxed),
        q => q,
    };
    if q == 0 {
        return;
    }
    let mut r = RENDER.lock().unwrap_or_else(|e| e.into_inner());
    if r.as_ref().map(|x| x.queue as usize != q).unwrap_or(true) {
        if FAILED.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
            return;
        }
        match Renderer::new(q as Unk) {
            Ok(x) => *r = Some(x),
            Err(e) => {
                crate::log::warn(&format!("overlay: renderer not available: {e}"));
                *FAILED.lock().unwrap_or_else(|e| e.into_inner()) = Some(e);
                return;
            }
        }
    }
    if let Err(e) = r.as_mut().unwrap().draw(sc, &dl, cap.as_deref()) {
        crate::log::warn(&format!("overlay: {e}"));
    } else {
        DRAWN.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draw_list_clips() {
        let mut d = DrawList::new(100, 100);
        d.rect(10.0, 10.0, 20.0, 20.0, 0xFF00_00FF);
        assert_eq!(d.verts.len(), 6);
        assert_eq!(d.verts[0].rgba, 0xFF00_0000 | 0x00FF_0000); // blue in the R|G<<8|B<<16 order
        d.set_clip(0.0, 0.0, 15.0, 100.0);
        d.rect(10.0, 10.0, 20.0, 20.0, 0xFFFF_FFFF);
        assert_eq!(d.verts[7].x, 15.0);
        d.set_clip(50.0, 50.0, 10.0, 10.0);
        let n = d.verts.len();
        d.rect(0.0, 0.0, 10.0, 10.0, 0xFFFF_FFFF);
        assert_eq!(d.verts.len(), n, "fully clipped");
        d.set_clip(0.0, 0.0, 0.0, 0.0);
        let w = d.text(0.0, 0.0, 2.0, 0xFFFF_FFFF, "Hi");
        assert!(w > 10.0);
    }
}
