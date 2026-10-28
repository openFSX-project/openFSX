//! Engine record loading of sim1.dll: the chunk reader 0x20AF1FC0 and the piston engine
//! loader 0x20B4F1C0.

/// sim1 0x20AF1FC0 (`__fastcall`: ECX = file, EDX = id, stack = destination, size): scans a
/// chunk file of `[id: u32][length: u32][data]` records for `id` and returns the first
/// `size` bytes of its data (zero-padded when the chunk is shorter). `None` when the
/// chunk is missing or truncated. Read from the assembly only (the original goes through
/// file imports and is not emulated).
pub fn read_chunk_20af1fc0(file: &[u8], id: u32, size: usize) -> Option<Vec<u8>> {
    let mut at = 0usize;
    loop {
        let tag = u32::from_le_bytes(file.get(at..at + 4)?.try_into().ok()?);
        let length = u32::from_le_bytes(file.get(at + 4..at + 8)?.try_into().ok()?) as usize;
        at += 8;
        if tag == id {
            let take = length.min(size);
            let mut out = vec![0u8; size];
            out[..take].copy_from_slice(file.get(at..at + take)?);
            return Some(out);
        }
        at = at.checked_add(length)?;
    }
}

/// One store of sim1 0x20B4F1C0 into the engine object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Store {
    Word(usize, u32),
    Half(usize, u16),
    Double(usize, f64),
}

/// Scale of the manifold-pressure records (the writer 0x20B4EF20 multiplies back by
/// 0.01413855).
pub const MANIFOLD_PRESSURE_SCALE: f64 = 70.7286096;

/// sim1 0x20B4F1C0, record decoding only: current format 0x505 (0x74 bytes) when
/// present, otherwise the legacy 0x500 record (0x44 bytes, whose values then feed the
/// 0x20B4FF10 initializer). Returns the stores into the engine object in the original's
/// order; `None` when neither record exists. Record 0x505 layout (packed): +0x00
/// displacement in^3, +0x08 compression ratio, +0x10 cylinders u32, +0x14 max rpm, +0x1C
/// max hp, +0x24 fuel metering u32, +0x28 cooling u32, +0x2C starter torque, +0x34
/// turbocharged u32, +0x38 max design MP, +0x40 min design MP, +0x48 (object +0x1B4),
/// +0x50 critical altitude, +0x58 emergency boost type u32, +0x5C emergency boost gain
/// offset, +0x64 emergency boost MP offset, +0x6C auto mixture u32, +0x70 auto ignition u32.
pub fn load_engine_20b4f1c0(rec505: Option<&[u8]>, rec500: Option<&[u8]>) -> Option<Vec<Store>> {
    let d = |r: &[u8], o: usize| f64::from_le_bytes(r[o..o + 8].try_into().unwrap());
    let w = |r: &[u8], o: usize| u32::from_le_bytes(r[o..o + 4].try_into().unwrap());
    let h = |r: &[u8], o: usize| u16::from_le_bytes(r[o..o + 2].try_into().unwrap());
    if let Some(r) = rec505 {
        assert!(r.len() >= 0x74);
        let s = MANIFOLD_PRESSURE_SCALE;
        return Some(vec![
            Store::Word(0x170, w(r, 0x10)),
            Store::Word(0x188, w(r, 0x24)),
            Store::Double(0x160, d(r, 0) * 0.0005787037037037037),
            Store::Double(0x168, d(r, 8)),
            Store::Double(0x178, d(r, 0x14)),
            Store::Double(0x180, d(r, 0x1c)),
            Store::Double(0x190, d(r, 0x2c)),
            Store::Word(0x18c, w(r, 0x28)),
            Store::Word(0x1a0, w(r, 0x34)),
            Store::Double(0x1a4, d(r, 0x38) * s),
            Store::Word(0x1c4, w(r, 0x58)),
            Store::Word(0x1d8, w(r, 0x6c)),
            Store::Double(0x1ac, d(r, 0x40) * s),
            Store::Double(0x1b4, d(r, 0x48)),
            Store::Double(0x1bc, d(r, 0x50)),
            Store::Double(0x1d0, d(r, 0x5c)),
            Store::Word(0x1e0, w(r, 0x70)),
            Store::Double(0x1c8, d(r, 0x64) * s),
        ]);
    }
    let r = rec500?;
    assert!(r.len() >= 0x44);
    Some(vec![
        Store::Half(0x10c, h(r, 0x0e)),
        Store::Half(0x10e, h(r, 0x14)),
        Store::Half(0x116, h(r, 0x38)),
        Store::Word(0x110, u32::from(h(r, 0x1e))),
        Store::Word(0x208, u32::from(h(r, 0x3c))),
        Store::Half(0x114, h(r, 0x2e)),
        Store::Double(0x20c, f64::from(h(r, 0x22)) * 0.08333333333333333),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_reader_skips_other_ids_and_pads_short_chunks() {
        let mut file = Vec::new();
        for (id, data) in [(1u32, vec![9u8; 3]), (0x500, vec![1, 2, 3])] {
            file.extend(id.to_le_bytes());
            file.extend((data.len() as u32).to_le_bytes());
            file.extend(data);
        }
        assert_eq!(read_chunk_20af1fc0(&file, 0x500, 5), Some(vec![1, 2, 3, 0, 0]));
        assert_eq!(read_chunk_20af1fc0(&file, 0x500, 2), Some(vec![1, 2]));
        assert_eq!(read_chunk_20af1fc0(&file, 0x505, 4), None);
    }
}
