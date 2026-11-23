//! Windows FFI used by gta-vi-scripthook (hand-written from Microsoft Learn): kernel32, user32,
//! D3D12, DXGI and the HLSL compiler. COM methods are called through vtable slots ([`vcall`]),
//! slot numbers as in the public SDK headers.

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, clippy::upper_case_acronyms, clippy::missing_safety_doc)]

use std::ffi::c_void;

pub type HANDLE = *mut c_void;
pub type HMODULE = *mut c_void;
pub type HWND = *mut c_void;
pub type BOOL = i32;
pub type HRESULT = i32;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GUID {
    pub d1: u32,
    pub d2: u16,
    pub d3: u16,
    pub d4: [u8; 8],
}

impl GUID {
    pub const fn from_u128(v: u128) -> GUID {
        GUID { d1: (v >> 96) as u32, d2: (v >> 80) as u16, d3: (v >> 64) as u16, d4: (v as u64).to_be_bytes() }
    }
}

// ---- kernel32 ------------------------------------------------------------------------------

pub const MEM_COMMIT: u32 = 0x1000;
pub const MEM_RESERVE: u32 = 0x2000;
pub const MEM_RELEASE: u32 = 0x8000;
pub const MEM_FREE: u32 = 0x10000;
pub const MEM_IMAGE: u32 = 0x100_0000;
pub const PAGE_NOACCESS: u32 = 0x01;
pub const PAGE_READONLY: u32 = 0x02;
pub const PAGE_READWRITE: u32 = 0x04;
pub const PAGE_WRITECOPY: u32 = 0x08;
pub const PAGE_EXECUTE: u32 = 0x10;
pub const PAGE_EXECUTE_READ: u32 = 0x20;
pub const PAGE_EXECUTE_READWRITE: u32 = 0x40;
pub const PAGE_EXECUTE_WRITECOPY: u32 = 0x80;
pub const PAGE_GUARD: u32 = 0x100;
pub const TH32CS_SNAPTHREAD: u32 = 0x4;
pub const THREAD_ALL_ACCESS: u32 = 0x1F_FFFF;
pub const INFINITE: u32 = 0xFFFF_FFFF;
pub const CREATE_SUSPENDED: u32 = 0x4;
pub const PROCESS_ALL_ACCESS: u32 = 0x1F_FFFF;
pub const CONTEXT_CONTROL: u32 = 0x0010_0001;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct MEMORY_BASIC_INFORMATION {
    pub BaseAddress: usize,
    pub AllocationBase: usize,
    pub AllocationProtect: u32,
    pub PartitionId: u16,
    pub RegionSize: usize,
    pub State: u32,
    pub Protect: u32,
    pub Type: u32,
}

#[repr(C)]
pub struct THREADENTRY32 {
    pub dwSize: u32,
    pub cntUsage: u32,
    pub th32ThreadID: u32,
    pub th32OwnerProcessID: u32,
    pub tpBasePri: i32,
    pub tpDeltaPri: i32,
    pub dwFlags: u32,
}

#[repr(C)]
#[derive(Default)]
pub struct MODULEINFO {
    pub lpBaseOfDll: usize,
    pub SizeOfImage: u32,
    pub EntryPoint: usize,
}

#[repr(C)]
pub struct STARTUPINFOW {
    pub cb: u32,
    pub lpReserved: *mut u16,
    pub lpDesktop: *mut u16,
    pub lpTitle: *mut u16,
    pub dwX: u32,
    pub dwY: u32,
    pub dwXSize: u32,
    pub dwYSize: u32,
    pub dwXCountChars: u32,
    pub dwYCountChars: u32,
    pub dwFillAttribute: u32,
    pub dwFlags: u32,
    pub wShowWindow: u16,
    pub cbReserved2: u16,
    pub lpReserved2: *mut u8,
    pub hStdInput: HANDLE,
    pub hStdOutput: HANDLE,
    pub hStdError: HANDLE,
}

#[repr(C)]
pub struct PROCESS_INFORMATION {
    pub hProcess: HANDLE,
    pub hThread: HANDLE,
    pub dwProcessId: u32,
    pub dwThreadId: u32,
}

pub type ThreadProc = unsafe extern "system" fn(*mut c_void) -> u32;
pub type FiberProc = unsafe extern "system" fn(*mut c_void);

#[link(name = "kernel32")]
extern "system" {
    pub fn VirtualAlloc(addr: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void;
    pub fn VirtualFree(addr: *mut c_void, size: usize, kind: u32) -> BOOL;
    pub fn VirtualProtect(addr: *mut c_void, size: usize, protect: u32, old: *mut u32) -> BOOL;
    pub fn VirtualQuery(addr: *const c_void, info: *mut MEMORY_BASIC_INFORMATION, len: usize) -> usize;
    pub fn FlushInstructionCache(process: HANDLE, addr: *const c_void, size: usize) -> BOOL;
    pub fn GetCurrentProcess() -> HANDLE;
    pub fn GetCurrentProcessId() -> u32;
    pub fn GetCurrentThreadId() -> u32;
    pub fn GetCurrentThread() -> HANDLE;
    pub fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> HANDLE;
    pub fn Thread32First(snap: HANDLE, te: *mut THREADENTRY32) -> BOOL;
    pub fn Thread32Next(snap: HANDLE, te: *mut THREADENTRY32) -> BOOL;
    pub fn OpenThread(access: u32, inherit: BOOL, tid: u32) -> HANDLE;
    pub fn SuspendThread(h: HANDLE) -> u32;
    pub fn ResumeThread(h: HANDLE) -> u32;
    pub fn GetThreadContext(h: HANDLE, ctx: *mut c_void) -> BOOL;
    pub fn CloseHandle(h: HANDLE) -> BOOL;
    pub fn CreateThread(attrs: *mut c_void, stack: usize, start: ThreadProc, param: *mut c_void, flags: u32, tid: *mut u32) -> HANDLE;
    pub fn Sleep(ms: u32);
    pub fn GetModuleHandleW(name: *const u16) -> HMODULE;
    pub fn GetModuleHandleExW(flags: u32, name: *const u16, out: *mut HMODULE) -> BOOL;
    pub fn GetModuleFileNameW(m: HMODULE, out: *mut u16, size: u32) -> u32;
    pub fn LoadLibraryW(name: *const u16) -> HMODULE;
    pub fn LoadLibraryExW(name: *const u16, file: HANDLE, flags: u32) -> HMODULE;
    pub fn GetProcAddress(m: HMODULE, name: *const u8) -> *mut c_void;
    pub fn FreeLibrary(m: HMODULE) -> BOOL;
    pub fn K32EnumProcessModules(p: HANDLE, mods: *mut HMODULE, cb: u32, needed: *mut u32) -> BOOL;
    pub fn K32GetModuleInformation(p: HANDLE, m: HMODULE, info: *mut MODULEINFO, cb: u32) -> BOOL;
    pub fn K32GetModuleBaseNameW(p: HANDLE, m: HMODULE, out: *mut u16, size: u32) -> u32;
    pub fn ReadProcessMemory(p: HANDLE, addr: *const c_void, out: *mut c_void, size: usize, read: *mut usize) -> BOOL;
    pub fn WriteProcessMemory(p: HANDLE, addr: *mut c_void, data: *const c_void, size: usize, written: *mut usize) -> BOOL;
    pub fn VirtualAllocEx(p: HANDLE, addr: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void;
    pub fn VirtualFreeEx(p: HANDLE, addr: *mut c_void, size: usize, kind: u32) -> BOOL;
    pub fn CreateRemoteThread(p: HANDLE, attrs: *mut c_void, stack: usize, start: *mut c_void, param: *mut c_void, flags: u32, tid: *mut u32) -> HANDLE;
    pub fn OpenProcess(access: u32, inherit: BOOL, pid: u32) -> HANDLE;
    pub fn GetExitCodeThread(h: HANDLE, code: *mut u32) -> BOOL;
    pub fn GetExitCodeProcess(h: HANDLE, code: *mut u32) -> BOOL;
    pub fn CreateProcessW(app: *const u16, cmd: *mut u16, pa: *mut c_void, ta: *mut c_void, inherit: BOOL, flags: u32, env: *mut c_void, dir: *const u16, si: *mut STARTUPINFOW, pi: *mut PROCESS_INFORMATION) -> BOOL;
    pub fn WaitForSingleObject(h: HANDLE, ms: u32) -> u32;
    pub fn ConvertThreadToFiber(param: *mut c_void) -> *mut c_void;
    pub fn ConvertFiberToThread() -> BOOL;
    pub fn CreateFiber(stack: usize, start: FiberProc, param: *mut c_void) -> *mut c_void;
    pub fn DeleteFiber(f: *mut c_void);
    pub fn SwitchToFiber(f: *mut c_void);
    pub fn IsThreadAFiber() -> BOOL;
    pub fn CreateEventW(attrs: *mut c_void, manual: BOOL, initial: BOOL, name: *const u16) -> HANDLE;
    pub fn OutputDebugStringA(s: *const u8);
    pub fn QueryPerformanceCounter(out: *mut i64) -> BOOL;
    pub fn QueryPerformanceFrequency(out: *mut i64) -> BOOL;
    pub fn GetLastError() -> u32;
    pub fn DisableThreadLibraryCalls(m: HMODULE) -> BOOL;
    pub fn Process32FirstW(snap: HANDLE, pe: *mut PROCESSENTRY32W) -> BOOL;
    pub fn Process32NextW(snap: HANDLE, pe: *mut PROCESSENTRY32W) -> BOOL;
}

pub const TH32CS_SNAPPROCESS: u32 = 0x2;

#[repr(C)]
pub struct PROCESSENTRY32W {
    pub dwSize: u32,
    pub cntUsage: u32,
    pub th32ProcessID: u32,
    pub th32DefaultHeapID: usize,
    pub th32ModuleID: u32,
    pub cntThreads: u32,
    pub th32ParentProcessID: u32,
    pub pcPriClassBase: i32,
    pub dwFlags: u32,
    pub szExeFile: [u16; 260],
}

pub const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;

// ---- user32 --------------------------------------------------------------------------------

pub type WNDPROC = unsafe extern "system" fn(HWND, u32, usize, isize) -> isize;

#[repr(C)]
pub struct WNDCLASSEXW {
    pub cbSize: u32,
    pub style: u32,
    pub lpfnWndProc: WNDPROC,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: HMODULE,
    pub hIcon: *mut c_void,
    pub hCursor: *mut c_void,
    pub hbrBackground: *mut c_void,
    pub lpszMenuName: *const u16,
    pub lpszClassName: *const u16,
    pub hIconSm: *mut c_void,
}

#[repr(C)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: u32,
    pub wParam: usize,
    pub lParam: isize,
    pub time: u32,
    pub pt: [i32; 2],
    pub private: u32,
}

pub const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
pub const WS_POPUP: u32 = 0x8000_0000;
pub const WS_EX_TOOLWINDOW: u32 = 0x80;
pub const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
pub const SW_HIDE: i32 = 0;
pub const SW_SHOWNOACTIVATE: i32 = 4;
pub const PM_REMOVE: u32 = 1;

#[link(name = "user32")]
extern "system" {
    pub fn RegisterClassExW(c: *const WNDCLASSEXW) -> u16;
    pub fn CreateWindowExW(ex: u32, class: *const u16, title: *const u16, style: u32, x: i32, y: i32, w: i32, h: i32, parent: HWND, menu: *mut c_void, inst: HMODULE, param: *mut c_void) -> HWND;
    pub fn DefWindowProcW(h: HWND, m: u32, w: usize, l: isize) -> isize;
    pub fn DestroyWindow(h: HWND) -> BOOL;
    pub fn ShowWindow(h: HWND, cmd: i32) -> BOOL;
    pub fn PeekMessageW(m: *mut MSG, h: HWND, min: u32, max: u32, remove: u32) -> BOOL;
    pub fn TranslateMessage(m: *const MSG) -> BOOL;
    pub fn DispatchMessageW(m: *const MSG) -> isize;
    pub fn GetAsyncKeyState(vk: i32) -> i16;
    pub fn GetForegroundWindow() -> HWND;
    pub fn GetWindowThreadProcessId(h: HWND, pid: *mut u32) -> u32;
}

// ---- DXGI / D3D12 --------------------------------------------------------------------------

pub const IID_IDXGIFactory4: GUID = GUID::from_u128(0x1bc6ea02_ef36_464f_bf0c_21ca39e5168a);
pub const IID_IDXGISwapChain3: GUID = GUID::from_u128(0x94d99bdb_f1f8_4ab0_b236_7da0170edab1);
pub const IID_ID3D12Device: GUID = GUID::from_u128(0x189819f1_1db6_4b57_be54_1821339b85f7);
pub const IID_ID3D12CommandQueue: GUID = GUID::from_u128(0x0ec870a6_5d7e_4c22_8cfc_5baae07616ed);
pub const IID_ID3D12CommandAllocator: GUID = GUID::from_u128(0x6102dee4_af59_4b09_b999_b44d73f09b24);
pub const IID_ID3D12GraphicsCommandList: GUID = GUID::from_u128(0x5b160d0f_ac1b_4185_8ba8_b3ae42a5a455);
pub const IID_ID3D12DescriptorHeap: GUID = GUID::from_u128(0x8efb471d_616c_4f49_90f7_127bb763fa51);
pub const IID_ID3D12Fence: GUID = GUID::from_u128(0x0a753dcf_c4d8_4b91_adf6_be5a60d95a76);
pub const IID_ID3D12Resource: GUID = GUID::from_u128(0x696442be_a72e_4059_bc79_5b5c98040fad);
pub const IID_ID3D12RootSignature: GUID = GUID::from_u128(0xc54a6b66_72df_4ee8_8be5_a946a1429214);
pub const IID_ID3D12PipelineState: GUID = GUID::from_u128(0x765a30f3_f624_4c6f_a828_ace948622445);

pub const DXGI_FORMAT_R8G8B8A8_UNORM: u32 = 28;
pub const DXGI_FORMAT_B8G8R8A8_UNORM: u32 = 87;
pub const DXGI_FORMAT_R32G32_FLOAT: u32 = 16;
pub const DXGI_FORMAT_UNKNOWN: u32 = 0;
pub const DXGI_USAGE_RENDER_TARGET_OUTPUT: u32 = 0x20;
pub const DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL: u32 = 3;
pub const DXGI_SWAP_EFFECT_FLIP_DISCARD: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct DXGI_SWAP_CHAIN_DESC1 {
    pub Width: u32,
    pub Height: u32,
    pub Format: u32,
    pub Stereo: BOOL,
    pub SampleCount: u32,
    pub SampleQuality: u32,
    pub BufferUsage: u32,
    pub BufferCount: u32,
    pub Scaling: u32,
    pub SwapEffect: u32,
    pub AlphaMode: u32,
    pub Flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct D3D12_COMMAND_QUEUE_DESC {
    pub Type: u32,
    pub Priority: i32,
    pub Flags: u32,
    pub NodeMask: u32,
}

pub const D3D12_COMMAND_LIST_TYPE_DIRECT: u32 = 0;
pub const D3D12_DESCRIPTOR_HEAP_TYPE_RTV: u32 = 2;
pub const D3D12_RESOURCE_STATE_PRESENT: u32 = 0;
pub const D3D12_RESOURCE_STATE_RENDER_TARGET: u32 = 0x4;
pub const D3D12_RESOURCE_STATE_COPY_SOURCE: u32 = 0x800;
pub const D3D12_RESOURCE_STATE_GENERIC_READ: u32 = 0xAC3;
pub const D3D12_RESOURCE_STATE_COPY_DEST: u32 = 0x400;
pub const D3D12_HEAP_TYPE_UPLOAD: u32 = 2;
pub const D3D12_HEAP_TYPE_READBACK: u32 = 3;
pub const D3D12_RESOURCE_DIMENSION_BUFFER: u32 = 1;
pub const D3D12_TEXTURE_LAYOUT_ROW_MAJOR: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct D3D12_DESCRIPTOR_HEAP_DESC {
    pub Type: u32,
    pub NumDescriptors: u32,
    pub Flags: u32,
    pub NodeMask: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct D3D12_RESOURCE_TRANSITION_BARRIER {
    pub Type: u32,
    pub Flags: u32,
    pub pResource: *mut c_void,
    pub Subresource: u32,
    pub StateBefore: u32,
    pub StateAfter: u32,
    pub _pad: u32,
}

impl D3D12_RESOURCE_TRANSITION_BARRIER {
    pub fn new(res: *mut c_void, before: u32, after: u32) -> Self {
        D3D12_RESOURCE_TRANSITION_BARRIER { Type: 0, Flags: 0, pResource: res, Subresource: 0xFFFF_FFFF, StateBefore: before, StateAfter: after, _pad: 0 }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct D3D12_HEAP_PROPERTIES {
    pub Type: u32,
    pub CPUPageProperty: u32,
    pub MemoryPoolPreference: u32,
    pub CreationNodeMask: u32,
    pub VisibleNodeMask: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct D3D12_RESOURCE_DESC {
    pub Dimension: u32,
    pub Alignment: u64,
    pub Width: u64,
    pub Height: u32,
    pub DepthOrArraySize: u16,
    pub MipLevels: u16,
    pub Format: u32,
    pub SampleCount: u32,
    pub SampleQuality: u32,
    pub Layout: u32,
    pub Flags: u32,
}

impl D3D12_RESOURCE_DESC {
    pub fn buffer(size: u64) -> Self {
        D3D12_RESOURCE_DESC { Dimension: D3D12_RESOURCE_DIMENSION_BUFFER, Width: size, Height: 1, DepthOrArraySize: 1, MipLevels: 1, SampleCount: 1, Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR, ..Default::default() }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct D3D12_SHADER_BYTECODE {
    pub ptr: *const c_void,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct D3D12_RENDER_TARGET_BLEND_DESC {
    pub BlendEnable: BOOL,
    pub LogicOpEnable: BOOL,
    pub SrcBlend: u32,
    pub DestBlend: u32,
    pub BlendOp: u32,
    pub SrcBlendAlpha: u32,
    pub DestBlendAlpha: u32,
    pub BlendOpAlpha: u32,
    pub LogicOp: u32,
    pub RenderTargetWriteMask: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct D3D12_INPUT_ELEMENT_DESC {
    pub SemanticName: *const u8,
    pub SemanticIndex: u32,
    pub Format: u32,
    pub InputSlot: u32,
    pub AlignedByteOffset: u32,
    pub InputSlotClass: u32,
    pub InstanceDataStepRate: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct D3D12_GRAPHICS_PIPELINE_STATE_DESC {
    pub pRootSignature: *mut c_void,
    pub VS: D3D12_SHADER_BYTECODE,
    pub PS: D3D12_SHADER_BYTECODE,
    pub DS: D3D12_SHADER_BYTECODE,
    pub HS: D3D12_SHADER_BYTECODE,
    pub GS: D3D12_SHADER_BYTECODE,
    // stream output
    pub pSODeclaration: *const c_void,
    pub SONumEntries: u32,
    pub pBufferStrides: *const u32,
    pub SONumStrides: u32,
    pub RasterizedStream: u32,
    // blend
    pub AlphaToCoverageEnable: BOOL,
    pub IndependentBlendEnable: BOOL,
    pub RenderTarget: [D3D12_RENDER_TARGET_BLEND_DESC; 8],
    pub SampleMask: u32,
    // rasterizer
    pub FillMode: u32,
    pub CullMode: u32,
    pub FrontCounterClockwise: BOOL,
    pub DepthBias: i32,
    pub DepthBiasClamp: f32,
    pub SlopeScaledDepthBias: f32,
    pub DepthClipEnable: BOOL,
    pub MultisampleEnable: BOOL,
    pub AntialiasedLineEnable: BOOL,
    pub ForcedSampleCount: u32,
    pub ConservativeRaster: u32,
    // depth stencil
    pub DepthEnable: BOOL,
    pub DepthWriteMask: u32,
    pub DepthFunc: u32,
    pub StencilEnable: BOOL,
    pub StencilReadMask: u8,
    pub StencilWriteMask: u8,
    pub FrontFace: [u32; 4],
    pub BackFace: [u32; 4],
    // input layout
    pub pInputElementDescs: *const D3D12_INPUT_ELEMENT_DESC,
    pub NumElements: u32,
    pub _pad_il: u32,
    pub IBStripCutValue: u32,
    pub PrimitiveTopologyType: u32,
    pub NumRenderTargets: u32,
    pub RTVFormats: [u32; 8],
    pub DSVFormat: u32,
    pub SampleCount: u32,
    pub SampleQuality: u32,
    pub NodeMask: u32,
    pub CachedPSO: D3D12_SHADER_BYTECODE,
    pub Flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct D3D12_ROOT_PARAMETER {
    pub ParameterType: u32,
    pub _pad0: u32,
    /// union at offset 8: constants { register, space, count } or a root descriptor { register, space }
    pub a: u32,
    pub b: u32,
    pub c: u32,
    pub _pad1: u32,
    pub ShaderVisibility: u32,
    pub _pad2: u32,
}

#[repr(C)]
pub struct D3D12_ROOT_SIGNATURE_DESC {
    pub NumParameters: u32,
    pub pParameters: *const D3D12_ROOT_PARAMETER,
    pub NumStaticSamplers: u32,
    pub pStaticSamplers: *const c_void,
    pub Flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct D3D12_VERTEX_BUFFER_VIEW {
    pub BufferLocation: u64,
    pub SizeInBytes: u32,
    pub StrideInBytes: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
    pub Offset: u64,
    pub Format: u32,
    pub Width: u32,
    pub Height: u32,
    pub Depth: u32,
    pub RowPitch: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct D3D12_TEXTURE_COPY_LOCATION {
    pub pResource: *mut c_void,
    pub Type: u32,
    pub _pad: u32,
    pub Footprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
}

#[link(name = "d3d12")]
extern "system" {
    pub fn D3D12CreateDevice(adapter: *mut c_void, level: u32, iid: *const GUID, out: *mut *mut c_void) -> HRESULT;
    pub fn D3D12SerializeRootSignature(desc: *const D3D12_ROOT_SIGNATURE_DESC, version: u32, blob: *mut *mut c_void, err: *mut *mut c_void) -> HRESULT;
}

#[link(name = "dxgi")]
extern "system" {
    pub fn CreateDXGIFactory2(flags: u32, iid: *const GUID, out: *mut *mut c_void) -> HRESULT;
}

/// A COM interface pointer.
pub type Unk = *mut c_void;

/// The function in vtable slot `slot` of `obj`.
///
/// # Safety
/// `obj` must be a live COM object whose slot has the signature `F`.
pub unsafe fn vfn<F: Copy>(obj: Unk, slot: usize) -> F {
    let vt = *(obj as *const *const usize);
    std::mem::transmute_copy::<usize, F>(&*vt.add(slot))
}

/// Address in vtable slot `slot`.
///
/// # Safety
/// `obj` must be a live COM object.
pub unsafe fn vslot(obj: Unk, slot: usize) -> usize {
    *(*(obj as *const *const usize)).add(slot)
}

/// `IUnknown::Release` (null is ignored).
///
/// # Safety
/// `obj` must be a live COM object or null.
pub unsafe fn release(obj: Unk) {
    if !obj.is_null() {
        vfn::<unsafe extern "system" fn(Unk) -> u32>(obj, 2)(obj);
    }
}

/// `IUnknown::QueryInterface`.
///
/// # Safety
/// `obj` must be a live COM object.
pub unsafe fn query(obj: Unk, iid: &GUID) -> Option<Unk> {
    let mut out: Unk = std::ptr::null_mut();
    let hr = vfn::<unsafe extern "system" fn(Unk, *const GUID, *mut Unk) -> HRESULT>(obj, 0)(obj, iid, &mut out);
    (hr >= 0 && !out.is_null()).then_some(out)
}

/// `ID3DBlob` contents.
///
/// # Safety
/// `blob` must be a live ID3DBlob.
pub unsafe fn blob_bytes(blob: Unk) -> Vec<u8> {
    let p = vfn::<unsafe extern "system" fn(Unk) -> *const u8>(blob, 3)(blob);
    let n = vfn::<unsafe extern "system" fn(Unk) -> usize>(blob, 4)(blob);
    std::slice::from_raw_parts(p, n).to_vec()
}

/// `D3DCompile` from Windows' d3dcompiler_47.dll (loaded on first use).
pub fn d3d_compile(src: &str, entry: &str, target: &str) -> Result<Vec<u8>, String> {
    type Compile = unsafe extern "system" fn(*const c_void, usize, *const u8, *const c_void, *mut c_void, *const u8, *const u8, u32, u32, *mut Unk, *mut Unk) -> HRESULT;
    unsafe {
        let m = LoadLibraryW(wide("d3dcompiler_47.dll").as_ptr());
        if m.is_null() {
            return Err("d3dcompiler_47.dll not found".into());
        }
        let f = GetProcAddress(m, c"D3DCompile".as_ptr() as *const u8);
        if f.is_null() {
            return Err("D3DCompile not found".into());
        }
        let f: Compile = std::mem::transmute(f);
        let e = format!("{entry}\0");
        let t = format!("{target}\0");
        let (mut code, mut err): (Unk, Unk) = (std::ptr::null_mut(), std::ptr::null_mut());
        let hr = f(src.as_ptr() as *const c_void, src.len(), c"overlay".as_ptr() as *const u8, std::ptr::null(), std::ptr::null_mut(), e.as_ptr(), t.as_ptr(), 0, 0, &mut code, &mut err);
        if hr < 0 || code.is_null() {
            let msg = if err.is_null() { format!("D3DCompile failed {hr:#x}") } else { String::from_utf8_lossy(&blob_bytes(err)).into_owned() };
            release(err);
            return Err(msg);
        }
        let b = blob_bytes(code);
        release(code);
        release(err);
        Ok(b)
    }
}

/// UTF-16, NUL-terminated.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A UTF-16 buffer up to its NUL.
pub fn from_wide(w: &[u16]) -> String {
    let n = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..n])
}

/// Monotonic seconds.
pub fn now() -> f64 {
    let (mut c, mut f) = (0i64, 0i64);
    // SAFETY: out parameters.
    unsafe {
        QueryPerformanceCounter(&mut c);
        QueryPerformanceFrequency(&mut f);
    }
    if f == 0 {
        0.0
    } else {
        c as f64 / f as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layouts() {
        assert_eq!(std::mem::size_of::<D3D12_RESOURCE_DESC>(), 56);
        assert_eq!(std::mem::size_of::<D3D12_RESOURCE_TRANSITION_BARRIER>(), 32);
        assert_eq!(std::mem::size_of::<D3D12_ROOT_PARAMETER>(), 32);
        assert_eq!(std::mem::size_of::<D3D12_INPUT_ELEMENT_DESC>(), 32);
        assert_eq!(std::mem::size_of::<DXGI_SWAP_CHAIN_DESC1>(), 48);
        assert_eq!(std::mem::size_of::<MEMORY_BASIC_INFORMATION>(), 48);
        assert_eq!(std::mem::size_of::<D3D12_TEXTURE_COPY_LOCATION>(), 48);
        // the graphics pipeline description of the SDK headers is 656 bytes on x64
        assert_eq!(std::mem::size_of::<D3D12_GRAPHICS_PIPELINE_STATE_DESC>(), 656);
    }
}
