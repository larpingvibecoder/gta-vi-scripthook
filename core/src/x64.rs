//! An x86-64 instruction length decoder and relocator: enough to move the first instructions of
//! a function into a trampoline (prefixes, REX, VEX, EVEX, the one-byte / 0F / 0F38 / 0F3A maps,
//! ModRM / SIB / displacements / immediates, RIP-relative operands, relative branches).
//! Written from Intel's and AMD's public instruction set manuals.

/// What kind of control transfer an instruction is (for relocation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Other,
    /// jmp rel8 / rel32
    Jmp,
    /// jcc rel8 / rel32 (condition 0..15)
    Jcc(u8),
    /// call rel32
    Call,
    /// loop / loope / loopne / jrcxz: cannot be relocated
    Loop,
    /// ret, int3, jmp/call through registers or memory: ends a linear sequence
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Insn {
    pub len: usize,
    /// offset of a RIP-relative disp32 inside the instruction
    pub rip_disp: Option<usize>,
    pub flow: Flow,
    /// offset and size of a relative branch displacement
    pub rel: Option<(usize, usize)>,
}

fn has_modrm_1(op: u8) -> bool {
    matches!(op,
        0x00..=0x03 | 0x08..=0x0B | 0x10..=0x13 | 0x18..=0x1B | 0x20..=0x23 | 0x28..=0x2B | 0x30..=0x33 | 0x38..=0x3B |
        0x63 | 0x69 | 0x6B | 0x80..=0x8F | 0xC0 | 0xC1 | 0xC6 | 0xC7 | 0xD0..=0xD3 | 0xD8..=0xDF | 0xF6 | 0xF7 | 0xFE | 0xFF)
}

fn invalid_1(op: u8) -> bool {
    matches!(op, 0x06 | 0x07 | 0x0E | 0x16 | 0x17 | 0x1E | 0x1F | 0x27 | 0x2F | 0x37 | 0x3F | 0x60 | 0x61 | 0x9A | 0xCE | 0xD4 | 0xD5 | 0xD6 | 0xEA)
}

fn no_modrm_0f(op: u8) -> bool {
    matches!(op, 0x05..=0x09 | 0x0B | 0x0E | 0x30..=0x37 | 0x77 | 0x80..=0x8F | 0xA0..=0xA2 | 0xA8..=0xAA | 0xC8..=0xCF)
}

fn imm8_0f(op: u8) -> bool {
    matches!(op, 0x70..=0x73 | 0xA4 | 0xAC | 0xBA | 0xC2 | 0xC4..=0xC6)
}

/// Decodes the instruction at the start of `c`.
pub fn decode(c: &[u8]) -> Result<Insn, String> {
    let get = |i: usize| c.get(i).copied().ok_or_else(|| "truncated instruction".to_string());
    let mut i = 0;
    let (mut opsize16, mut addr32) = (false, false);
    // legacy prefixes
    loop {
        match get(i)? {
            0x66 => opsize16 = true,
            0x67 => addr32 = true,
            0xF0 | 0xF2 | 0xF3 | 0x2E | 0x36 | 0x3E | 0x26 | 0x64 | 0x65 => {}
            _ => break,
        }
        i += 1;
        if i > 14 {
            return Err("too many prefixes".into());
        }
    }
    let _ = addr32;
    let mut rex_w = false;
    let b0 = get(i)?;
    // VEX / EVEX
    if b0 == 0xC4 || b0 == 0xC5 || b0 == 0x62 {
        let (map, op_at) = match b0 {
            0xC5 => (1u8, i + 2),
            0xC4 => (get(i + 1)? & 0x1F, i + 3),
            _ => (get(i + 1)? & 0x03, i + 4),
        };
        let op = get(op_at)?;
        if map == 1 && op == 0x77 {
            return Ok(Insn { len: op_at + 1, rip_disp: None, flow: Flow::Other, rel: None }); // vzeroupper / vzeroall
        }
        let imm = match map {
            3 => 1,
            1 if imm8_0f(op) => 1,
            1 | 2 => 0,
            m => return Err(format!("unknown VEX/EVEX map {m}")),
        };
        let (mlen, rip) = modrm_len(c, op_at + 1)?;
        let len = op_at + 1 + mlen + imm;
        return Ok(Insn { len, rip_disp: rip, flow: Flow::Other, rel: None });
    }
    if (0x40..=0x4F).contains(&b0) {
        rex_w = b0 & 8 != 0;
        i += 1;
    }
    let op = get(i)?;
    i += 1;
    let z = if opsize16 { 2 } else { 4 };
    if op == 0x0F {
        let op2 = get(i)?;
        i += 1;
        match op2 {
            0x38 => {
                i += 1; // the opcode
                let (m, rip) = modrm_len(c, i)?;
                return Ok(Insn { len: i + m, rip_disp: rip, flow: Flow::Other, rel: None });
            }
            0x3A => {
                i += 1;
                let (m, rip) = modrm_len(c, i)?;
                return Ok(Insn { len: i + m + 1, rip_disp: rip, flow: Flow::Other, rel: None });
            }
            0x0F => return Err("3DNow! instructions are not supported".into()),
            0x80..=0x8F => return Ok(Insn { len: i + 4, rip_disp: None, flow: Flow::Jcc(op2 & 0xF), rel: Some((i, 4)) }),
            _ => {}
        }
        if no_modrm_0f(op2) {
            return Ok(Insn { len: i, rip_disp: None, flow: Flow::Other, rel: None });
        }
        let (m, rip) = modrm_len(c, i)?;
        let imm = if imm8_0f(op2) { 1 } else { 0 };
        return Ok(Insn { len: i + m + imm, rip_disp: rip, flow: Flow::Other, rel: None });
    }
    if invalid_1(op) {
        return Err(format!("opcode {op:#04x} is not valid in 64-bit mode"));
    }
    // branches
    match op {
        0x70..=0x7F => return Ok(Insn { len: i + 1, rip_disp: None, flow: Flow::Jcc(op & 0xF), rel: Some((i, 1)) }),
        0xEB => return Ok(Insn { len: i + 1, rip_disp: None, flow: Flow::Jmp, rel: Some((i, 1)) }),
        0xE9 => return Ok(Insn { len: i + 4, rip_disp: None, flow: Flow::Jmp, rel: Some((i, 4)) }),
        0xE8 => return Ok(Insn { len: i + 4, rip_disp: None, flow: Flow::Call, rel: Some((i, 4)) }),
        0xE0..=0xE3 => return Ok(Insn { len: i + 1, rip_disp: None, flow: Flow::Loop, rel: Some((i, 1)) }),
        0xC3 | 0xCB | 0xCC => return Ok(Insn { len: i, rip_disp: None, flow: Flow::End, rel: None }),
        0xC2 | 0xCA => return Ok(Insn { len: i + 2, rip_disp: None, flow: Flow::End, rel: None }),
        _ => {}
    }
    let mut imm = match op {
        0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x34 | 0x3C | 0x6A | 0x6B | 0x80 | 0x82 | 0x83 | 0xA8 | 0xB0..=0xB7 | 0xC0 | 0xC1 | 0xC6 | 0xCD | 0xE4..=0xE7 => 1,
        0x05 | 0x0D | 0x15 | 0x1D | 0x25 | 0x2D | 0x35 | 0x3D | 0x68 | 0x69 | 0x81 | 0xA9 | 0xC7 => z,
        0xB8..=0xBF => {
            if rex_w {
                8
            } else {
                z
            }
        }
        0xA0..=0xA3 => {
            if addr32 {
                4
            } else {
                8
            }
        }
        0xC8 => 3,
        _ => 0,
    };
    if has_modrm_1(op) {
        let modrm = get(i)?;
        let reg = (modrm >> 3) & 7;
        if op == 0xF6 && reg < 2 {
            imm = 1;
        }
        if op == 0xF7 && reg < 2 {
            imm = z;
        }
        let (m, rip) = modrm_len(c, i)?;
        let flow = if op == 0xFF && (2..=5).contains(&reg) { Flow::End } else { Flow::Other };
        return Ok(Insn { len: i + m + imm, rip_disp: rip, flow, rel: None });
    }
    Ok(Insn { len: i + imm, rip_disp: None, flow: Flow::Other, rel: None })
}

/// Length of ModRM + SIB + displacement at `at`, and the offset of a RIP-relative disp32.
fn modrm_len(c: &[u8], at: usize) -> Result<(usize, Option<usize>), String> {
    let m = *c.get(at).ok_or("truncated ModRM")?;
    let (md, rm) = (m >> 6, m & 7);
    if md == 3 {
        return Ok((1, None));
    }
    let mut n = 1;
    if rm == 4 {
        let sib = *c.get(at + 1).ok_or("truncated SIB")?;
        n += 1;
        if md == 0 && sib & 7 == 5 {
            return Ok((n + 4, None));
        }
    } else if md == 0 && rm == 5 {
        return Ok((5, Some(at + 1)));
    }
    n += match md {
        1 => 1,
        2 => 4,
        _ => 0,
    };
    Ok((n, None))
}

/// Instructions covering at least `min` bytes at the start of `code`.
pub fn cover(code: &[u8], min: usize) -> Result<Vec<Insn>, String> {
    let mut v = Vec::new();
    let mut at = 0;
    while at < min {
        let ins = decode(&code[at..]).map_err(|e| format!("at +{at}: {e}"))?;
        if ins.flow == Flow::End && at + ins.len < min {
            return Err(format!("the function ends after {} bytes (too short to hook)", at + ins.len));
        }
        at += ins.len;
        v.push(ins);
    }
    Ok(v)
}

/// `jmp [rip+0]; dq target` (14 bytes).
pub fn abs_jmp(target: u64) -> Vec<u8> {
    let mut v = vec![0xFF, 0x25, 0, 0, 0, 0];
    v.extend_from_slice(&target.to_le_bytes());
    v
}

/// Copies the instructions `insns` from `src` (address `from`) to code meant to run at `to`,
/// rewriting RIP-relative operands and relative branches. Returns the new bytes.
pub fn relocate(src: &[u8], insns: &[Insn], from: u64, to: u64) -> Result<Vec<u8>, String> {
    let total: usize = insns.iter().map(|i| i.len).sum();
    let mut out: Vec<u8> = Vec::new();
    let mut at = 0usize;
    for ins in insns {
        let bytes = &src[at..at + ins.len];
        let old_next = from + (at + ins.len) as u64;
        let new_at = to + out.len() as u64;
        match (ins.flow, ins.rel) {
            (Flow::Loop, _) => return Err("loop / jrcxz in the hooked bytes cannot be relocated".into()),
            (f @ (Flow::Jmp | Flow::Jcc(_) | Flow::Call), Some((off, size))) => {
                let d = if size == 1 { bytes[off] as i8 as i64 } else { i32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as i64 };
                let target = (old_next as i64 + d) as u64;
                if target >= from && target < from + total as u64 && target != from {
                    return Err("a branch into the hooked bytes cannot be relocated".into());
                }
                match f {
                    Flow::Jmp => out.extend(abs_jmp(target)),
                    Flow::Call => {
                        // call [rip+2]; jmp +8; dq target
                        out.extend_from_slice(&[0xFF, 0x15, 2, 0, 0, 0, 0xEB, 8]);
                        out.extend_from_slice(&target.to_le_bytes());
                    }
                    Flow::Jcc(cc) => {
                        // inverse condition over an absolute jump
                        out.extend_from_slice(&[0x70 | (cc ^ 1), 14]);
                        out.extend(abs_jmp(target));
                    }
                    _ => unreachable!(),
                }
            }
            _ => {
                let mut b = bytes.to_vec();
                if let Some(off) = ins.rip_disp {
                    let d = i32::from_le_bytes(b[off..off + 4].try_into().unwrap()) as i64;
                    let target = old_next as i64 + d;
                    let new_next = new_at as i64 + ins.len as i64;
                    let nd = target - new_next;
                    if nd < i32::MIN as i64 || nd > i32::MAX as i64 {
                        return Err("a RIP-relative operand is out of reach of the trampoline".into());
                    }
                    b[off..off + 4].copy_from_slice(&(nd as i32).to_le_bytes());
                }
                out.extend_from_slice(&b);
            }
        }
        at += ins.len;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn len(b: &[u8]) -> usize {
        decode(b).unwrap_or_else(|e| panic!("{b:02x?}: {e}")).len
    }

    #[test]
    fn lengths() {
        let cases: &[(&[u8], usize)] = &[
            (&[0x55], 1),                                     // push rbp
            (&[0x48, 0x89, 0xE5], 3),                         // mov rbp, rsp
            (&[0x48, 0x83, 0xEC, 0x28], 4),                   // sub rsp, 0x28
            (&[0x48, 0x81, 0xEC, 0x00, 0x01, 0, 0], 7),       // sub rsp, 0x100
            (&[0x48, 0x89, 0x5C, 0x24, 0x08], 5),             // mov [rsp+8], rbx
            (&[0x48, 0x8B, 0x05, 1, 2, 3, 4], 7),             // mov rax, [rip+x]
            (&[0x8B, 0x05, 1, 2, 3, 4], 6),                   // mov eax, [rip+x]
            (&[0x48, 0x8D, 0x0D, 1, 2, 3, 4], 7),             // lea rcx, [rip+x]
            (&[0x48, 0xB8, 1, 2, 3, 4, 5, 6, 7, 8], 10),      // mov rax, imm64
            (&[0xB8, 1, 2, 3, 4], 5),                         // mov eax, imm32
            (&[0x66, 0xB8, 1, 2], 4),                         // mov ax, imm16
            (&[0xC7, 0x44, 0x24, 0x10, 1, 2, 3, 4], 8),       // mov dword [rsp+16], imm32
            (&[0xC6, 0x05, 1, 2, 3, 4, 9], 7),                // mov byte [rip+x], imm8
            (&[0xF6, 0xC1, 1], 3),                            // test cl, 1
            (&[0xF7, 0xD8], 2),                               // neg eax
            (&[0x0F, 0x1F, 0x44, 0, 0], 5),                   // nop dword [rax+rax]
            (&[0x0F, 0x29, 0x74, 0x24, 0x20], 5),             // movaps [rsp+32], xmm6
            (&[0xF3, 0x0F, 0x10, 0x05, 1, 2, 3, 4], 8),       // movss xmm0, [rip+x]
            (&[0x66, 0x0F, 0x3A, 0x0F, 0xC1, 8], 6),          // palignr xmm0, xmm1, 8
            (&[0x66, 0x0F, 0x38, 0x00, 0xC1], 5),             // pshufb xmm0, xmm1
            (&[0xC5, 0xF8, 0x77], 3),                         // vzeroupper
            (&[0xC5, 0xFA, 0x10, 0x05, 1, 2, 3, 4], 8),       // vmovss xmm0, [rip+x]
            (&[0xC4, 0xE3, 0x79, 0x16, 0xC0, 1], 6),          // vpextrd eax, xmm0, 1
            (&[0x62, 0xF1, 0x7C, 0x48, 0x28, 0xC1], 6),       // vmovaps zmm0, zmm1
            (&[0xE8, 1, 2, 3, 4], 5),
            (&[0xE9, 1, 2, 3, 4], 5),
            (&[0xEB, 5], 2),
            (&[0x74, 5], 2),
            (&[0x0F, 0x84, 1, 2, 3, 4], 6),
            (&[0xFF, 0x25, 1, 2, 3, 4], 6),                   // jmp [rip+x]
            (&[0xC3], 1),
            (&[0x40, 0x53], 2),                               // push rbx with REX
            (&[0x65, 0x48, 0x8B, 0x04, 0x25, 0x30, 0, 0, 0], 9), // mov rax, gs:[0x30]
            (&[0x48, 0xA1, 1, 2, 3, 4, 5, 6, 7, 8], 10),      // mov rax, moffs64
            (&[0xF0, 0x0F, 0xB1, 0x0A], 4),                   // lock cmpxchg [rdx], ecx
            (&[0x4C, 0x8D, 0x44, 0x24, 0x40], 5),             // lea r8, [rsp+0x40]
            (&[0x41, 0xB9, 1, 0, 0, 0], 6),                   // mov r9d, 1
        ];
        for (b, n) in cases {
            assert_eq!(len(b), *n, "{b:02x?}");
        }
        assert_eq!(decode(&[0x48, 0x8B, 0x05, 1, 2, 3, 4]).unwrap().rip_disp, Some(3));
        assert!(decode(&[0x06]).is_err());
        assert_eq!(decode(&[0xE2, 0]).unwrap().flow, Flow::Loop);
    }

    #[test]
    fn relocation() {
        // lea rcx, [rip+0x10]; jz +0x40; call +0x100; jmp +0x20
        let code = [0x48, 0x8D, 0x0D, 0x10, 0, 0, 0, 0x74, 0x40, 0xE8, 0, 1, 0, 0, 0xEB, 0x20];
        let insns = cover(&code, 16).unwrap();
        assert_eq!(insns.len(), 4);
        let from = 0x1_0000_0000u64;
        let to = 0x1_0000_8000u64;
        let out = relocate(&code, &insns, from, to).unwrap();
        // lea re-based: target was from+7+0x10
        let d = i32::from_le_bytes(out[3..7].try_into().unwrap()) as i64;
        assert_eq!((to as i64 + 7 + d) as u64, from + 7 + 0x10);
        // jz → jnz +14 over an absolute jump to from+9+0x40
        assert_eq!(&out[7..9], &[0x75, 14]);
        assert_eq!(u64::from_le_bytes(out[15..23].try_into().unwrap()), from + 9 + 0x40);
        // call → call [rip+2]; jmp +8; dq
        assert_eq!(&out[23..31], &[0xFF, 0x15, 2, 0, 0, 0, 0xEB, 8]);
        assert_eq!(u64::from_le_bytes(out[31..39].try_into().unwrap()), from + 14 + 0x100);
        assert_eq!(u64::from_le_bytes(out[45..53].try_into().unwrap()), from + 16 + 0x20);
        // out of reach
        assert!(relocate(&code[..7], &insns[..1], from, from + (1 << 33)).is_err());
        assert!(cover(&[0xC3, 0x90, 0x90, 0x90, 0x90], 5).is_err());
    }
}
