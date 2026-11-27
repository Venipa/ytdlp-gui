//! Progress of an aria2c download, read from its `.aria2` control file (yt-dlp gives aria2c no
//! progress channel). Format: aria2 manual, "Control File (*.aria2) Format".

/// Size of one block in an in-flight piece's bitfield.
const BLOCK: u64 = 16 * 1024;

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(head)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_be_bytes(self.take(8)?.try_into().ok()?))
    }
}

fn ones(bits: &[u8]) -> u64 {
    bits.iter().map(|b| u64::from(b.count_ones())).sum()
}

/// `(bytes done, total bytes)` from a control file; `None` when it is not one aria2 1.x writes
/// (version 1, big-endian) or it is cut short (aria2 rewrites it while we read).
pub fn control_progress(file: &[u8]) -> Option<(u64, u64)> {
    let mut r = Reader(file);
    if r.take(2)? != [0, 1] {
        return None;
    }
    r.take(4)?; // extension flags
    let hash = r.u32()? as usize;
    r.take(hash)?;
    let piece = u64::from(r.u32()?);
    let total = r.u64()?;
    r.u64()?; // uploaded
    let field_len = r.u32()? as usize;
    let field = r.take(field_len)?;
    if piece == 0 || total == 0 {
        return None;
    }
    let pieces = total.div_ceil(piece);
    let mut done = ones(field) * piece;
    // the last piece is short: count only what it holds
    let last = (pieces - 1) as usize;
    if field
        .get(last / 8)
        .is_some_and(|b| b & (0x80 >> (last % 8)) != 0)
    {
        done -= pieces * piece - total;
    }
    for _ in 0..r.u32()? {
        r.u32()?; // index
        let len = u64::from(r.u32()?);
        let blocks = r.u32()? as usize;
        done += (ones(r.take(blocks)?) * BLOCK).min(len);
    }
    Some((done.min(total), total))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A control file for `total` bytes in `piece`-byte pieces.
    fn file(piece: u32, total: u64, field: &[u8], inflight: &[(u32, u32, &[u8])]) -> Vec<u8> {
        let mut v = vec![0, 1, 0, 0, 0, 0];
        v.extend(0u32.to_be_bytes()); // no info hash for http
        v.extend(piece.to_be_bytes());
        v.extend(total.to_be_bytes());
        v.extend(0u64.to_be_bytes());
        v.extend((field.len() as u32).to_be_bytes());
        v.extend(field);
        v.extend((inflight.len() as u32).to_be_bytes());
        for (index, len, blocks) in inflight {
            v.extend(index.to_be_bytes());
            v.extend(len.to_be_bytes());
            v.extend((blocks.len() as u32).to_be_bytes());
            v.extend(*blocks);
        }
        v
    }

    const MIB: u32 = 1 << 20;

    #[test]
    fn counts_whole_pieces_and_the_short_last_one() {
        // 2.5 MiB: three pieces, the last half full
        let total = 5 * u64::from(MIB) / 2;
        assert_eq!(
            control_progress(&file(MIB, total, &[0b1000_0000], &[])),
            Some((u64::from(MIB), total))
        );
        assert_eq!(
            control_progress(&file(MIB, total, &[0b1110_0000], &[])),
            Some((total, total))
        );
        assert_eq!(
            control_progress(&file(MIB, total, &[0b0010_0000], &[])),
            Some((u64::from(MIB) / 2, total))
        );
    }

    #[test]
    fn adds_blocks_of_pieces_in_flight() {
        let total = 4 * u64::from(MIB);
        // piece 0 done, piece 1 has 3 of its 64 blocks
        let blocks = [0b1110_0000, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(
            control_progress(&file(MIB, total, &[0b1000_0000], &[(1, MIB, &blocks)])),
            Some((u64::from(MIB) + 3 * BLOCK, total))
        );
    }

    #[test]
    fn rejects_other_versions_and_cut_files() {
        let good = file(MIB, 2 * u64::from(MIB), &[0b1000_0000], &[]);
        let mut legacy = good.clone();
        legacy[1] = 0;
        assert_eq!(control_progress(&legacy), None);
        assert_eq!(control_progress(&good[..good.len() - 3]), None);
        assert_eq!(control_progress(&[]), None);
    }
}
