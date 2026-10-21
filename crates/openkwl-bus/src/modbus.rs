//! Modbus RTU master frames: the three function codes the stock controller uses toward its
//! modules (03 read holding registers, 04 read input registers, 16 write multiple registers),
//! the CRC, and checking an answer against the request.

/// Most registers in one request (Modbus limit for function 03/04).
pub const MAX_READ: u16 = 125;
/// Most registers in one write (function 16).
pub const MAX_WRITE: usize = 123;
/// Longest RTU frame.
pub const FRAME_MAX: usize = 256;

pub const READ_HOLDING: u8 = 0x03;
pub const READ_INPUT: u8 = 0x04;
pub const WRITE_MULTIPLE: u8 = 0x10;

/// CRC-16/MODBUS (poly 0xA001 reflected, init 0xFFFF), sent low byte first.
pub fn crc(data: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;
    for &b in data {
        crc ^= u16::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xA001 } else { crc >> 1 };
        }
    }
    crc
}

/// What is wrong with an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Shorter than a frame, or not the length the request implies.
    Length,
    Crc,
    /// From another slave, or for another function.
    Mismatch,
    /// The slave answered with an exception code.
    Exception(u8),
    /// The request does not fit Modbus limits or the buffer.
    Request,
}

/// Appends the CRC; returns the frame length.
fn finish(out: &mut [u8], len: usize) -> usize {
    let c = crc(&out[..len]);
    out[len..len + 2].copy_from_slice(&c.to_le_bytes());
    len + 2
}

/// Function 03 or 04: read `count` registers from `start`.
pub fn read(out: &mut [u8; FRAME_MAX], slave: u8, function: u8, start: u16, count: u16) -> Result<usize, Error> {
    if !(1..=MAX_READ).contains(&count) || !matches!(function, READ_HOLDING | READ_INPUT) {
        return Err(Error::Request);
    }
    out[0] = slave;
    out[1] = function;
    out[2..4].copy_from_slice(&start.to_be_bytes());
    out[4..6].copy_from_slice(&count.to_be_bytes());
    Ok(finish(out, 6))
}

/// Function 16: write `values` from register `start` (big-endian per register on the wire).
pub fn write_multiple(out: &mut [u8; FRAME_MAX], slave: u8, start: u16, values: &[u16]) -> Result<usize, Error> {
    if values.is_empty() || values.len() > MAX_WRITE {
        return Err(Error::Request);
    }
    out[0] = slave;
    out[1] = WRITE_MULTIPLE;
    out[2..4].copy_from_slice(&start.to_be_bytes());
    out[4..6].copy_from_slice(&(values.len() as u16).to_be_bytes());
    out[6] = (values.len() * 2) as u8;
    for (i, v) in values.iter().enumerate() {
        out[7 + 2 * i..9 + 2 * i].copy_from_slice(&v.to_be_bytes());
    }
    Ok(finish(out, 7 + 2 * values.len()))
}

/// Checks length, CRC, slave and function (or an exception for it); returns the PDU data
/// after the function code.
fn check(answer: &[u8], slave: u8, function: u8) -> Result<&[u8], Error> {
    if answer.len() < 5 {
        return Err(Error::Length);
    }
    let (frame, tail) = answer.split_at(answer.len() - 2);
    if crc(frame).to_le_bytes() != [tail[0], tail[1]] {
        return Err(Error::Crc);
    }
    if frame[0] != slave {
        return Err(Error::Mismatch);
    }
    if frame[1] == function | 0x80 {
        return Err(Error::Exception(frame[2]));
    }
    if frame[1] != function {
        return Err(Error::Mismatch);
    }
    Ok(&frame[2..])
}

/// The registers of a function 03/04 answer, into `regs` (as many as were requested).
pub fn read_answer(answer: &[u8], slave: u8, function: u8, regs: &mut [u16]) -> Result<(), Error> {
    let data = check(answer, slave, function)?;
    let n = usize::from(*data.first().ok_or(Error::Length)?);
    if n != regs.len() * 2 || data.len() != 1 + n {
        return Err(Error::Length);
    }
    for (i, r) in regs.iter_mut().enumerate() {
        *r = u16::from_be_bytes([data[1 + 2 * i], data[2 + 2 * i]]);
    }
    Ok(())
}

/// A function 16 answer echoes start and count.
pub fn write_answer(answer: &[u8], slave: u8, start: u16, count: u16) -> Result<(), Error> {
    let data = check(answer, slave, WRITE_MULTIPLE)?;
    if data.len() != 4 {
        return Err(Error::Length);
    }
    if data[..2] != start.to_be_bytes() || data[2..4] != count.to_be_bytes() {
        return Err(Error::Mismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_the_reference() {
        // Modbus spec example: 01 03 00 00 00 0A → CRC C5 CD on the wire.
        assert_eq!(crc(&[0x01, 0x03, 0x00, 0x00, 0x00, 0x0A]).to_le_bytes(), [0xC5, 0xCD]);
        // CRC-16/MODBUS check value.
        assert_eq!(crc(b"123456789"), 0x4B37);
    }

    #[test]
    fn requests() {
        let mut f = [0u8; FRAME_MAX];
        let n = read(&mut f, 0x01, READ_HOLDING, 0, 10).unwrap();
        assert_eq!(&f[..n], &[0x01, 0x03, 0x00, 0x00, 0x00, 0x0A, 0xC5, 0xCD]);
        let n = read(&mut f, 0x14, READ_INPUT, 0, 16).unwrap();
        assert_eq!(&f[..6], &[0x14, 0x04, 0x00, 0x00, 0x00, 0x10]);
        assert_eq!(crc(&f[..n]), 0, "a frame with its CRC checks to 0");
        let n = write_multiple(&mut f, 0x11, 1, &[0x000A, 0x0102]).unwrap();
        // Modbus spec example for function 16 (without CRC).
        assert_eq!(&f[..n - 2], &[0x11, 0x10, 0x00, 0x01, 0x00, 0x02, 0x04, 0x00, 0x0A, 0x01, 0x02]);
        assert_eq!(read(&mut f, 1, READ_INPUT, 0, 0), Err(Error::Request));
        assert_eq!(read(&mut f, 1, 0x05, 0, 1), Err(Error::Request));
        assert_eq!(write_multiple(&mut f, 1, 0, &[0; 124]), Err(Error::Request));
    }

    fn with_crc(frame: &[u8]) -> ([u8; FRAME_MAX], usize) {
        let mut f = [0u8; FRAME_MAX];
        f[..frame.len()].copy_from_slice(frame);
        let n = finish(&mut f, frame.len());
        (f, n)
    }

    #[test]
    fn answers() {
        let (f, n) = with_crc(&[0x14, 0x04, 0x04, 0x00, 0x01, 0x80, 0x03]);
        let mut regs = [0u16; 2];
        read_answer(&f[..n], 0x14, READ_INPUT, &mut regs).unwrap();
        assert_eq!(regs, [0x0001, 0x8003]);
        let mut three = [0u16; 3];
        assert_eq!(read_answer(&f[..n], 0x14, READ_INPUT, &mut three), Err(Error::Length));
        assert_eq!(read_answer(&f[..n], 0x15, READ_INPUT, &mut regs), Err(Error::Mismatch));
        let mut bad = f;
        bad[3] ^= 1;
        assert_eq!(read_answer(&bad[..n], 0x14, READ_INPUT, &mut regs), Err(Error::Crc));
        let (e, n) = with_crc(&[0x14, 0x84, 0x02]);
        assert_eq!(read_answer(&e[..n], 0x14, READ_INPUT, &mut regs), Err(Error::Exception(2)));
        let (w, n) = with_crc(&[0x14, 0x10, 0x00, 0x00, 0x00, 0x16]);
        assert_eq!(write_answer(&w[..n], 0x14, 0, 0x16), Ok(()));
        assert_eq!(write_answer(&w[..n], 0x14, 0, 0x15), Err(Error::Mismatch));
        assert_eq!(read_answer(&[1, 2], 1, READ_INPUT, &mut regs), Err(Error::Length));
    }
}
