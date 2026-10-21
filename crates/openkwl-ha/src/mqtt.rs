//! Minimal MQTT 3.1.1 client packets (QoS 0 publish/subscribe), enough for Home Assistant:
//! connect with a last will, publish retained discovery/state/availability, subscribe to the
//! command topics, keep-alive. Encoding into caller buffers; no allocation, host-tested.

/// Default broker port.
pub const PORT: u16 = 1883;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The output buffer is too small.
    BufferFull,
    /// Broken remaining-length field (the byte stream cannot be followed).
    Malformed,
}

struct Out<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl Out<'_> {
    fn put(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let end = self.len + bytes.len();
        self.buf.get_mut(self.len..end).ok_or(Error::BufferFull)?.copy_from_slice(bytes);
        self.len = end;
        Ok(())
    }

    fn u16(&mut self, v: u16) -> Result<(), Error> {
        self.put(&v.to_be_bytes())
    }

    fn str(&mut self, s: &[u8]) -> Result<(), Error> {
        self.u16(u16::try_from(s.len()).map_err(|_| Error::BufferFull)?)?;
        self.put(s)
    }
}

/// Writes fixed header + body; `body` writes into the buffer after a 4-byte gap that is then
/// closed up to the real length of the remaining-length field.
fn packet(buf: &mut [u8], first: u8, body: impl FnOnce(&mut Out<'_>) -> Result<(), Error>) -> Result<usize, Error> {
    const GAP: usize = 5;
    let mut out = Out { buf, len: GAP };
    body(&mut out)?;
    let remaining = out.len - GAP;
    let mut header = [0u8; GAP];
    header[0] = first;
    let mut n = 1;
    let mut x = remaining;
    loop {
        let mut byte = (x % 128) as u8;
        x /= 128;
        if x > 0 {
            byte |= 0x80;
        }
        header[n] = byte;
        n += 1;
        if x == 0 {
            break;
        }
        if n == GAP {
            return Err(Error::BufferFull);
        }
    }
    let buf = out.buf;
    buf.copy_within(GAP..GAP + remaining, n);
    buf[..n].copy_from_slice(&header[..n]);
    Ok(n + remaining)
}

/// Last will: published by the broker when the device disappears (availability `offline`).
#[derive(Debug, Clone, Copy)]
pub struct Will<'a> {
    pub topic: &'a str,
    pub payload: &'a [u8],
    pub retain: bool,
}

pub struct Connect<'a> {
    pub client_id: &'a str,
    pub username: Option<&'a str>,
    pub password: Option<&'a [u8]>,
    pub will: Option<Will<'a>>,
    pub keep_alive_s: u16,
}

pub fn connect(buf: &mut [u8], c: &Connect<'_>) -> Result<usize, Error> {
    packet(buf, 0x10, |o| {
        o.str(b"MQTT")?;
        o.put(&[4])?; // protocol level 3.1.1
        let mut flags = 0x02; // clean session
        if let Some(w) = &c.will {
            flags |= 0x04 | if w.retain { 0x20 } else { 0 };
        }
        if c.username.is_some() {
            flags |= 0x80;
        }
        if c.password.is_some() {
            flags |= 0x40;
        }
        o.put(&[flags])?;
        o.u16(c.keep_alive_s)?;
        o.str(c.client_id.as_bytes())?;
        if let Some(w) = &c.will {
            o.str(w.topic.as_bytes())?;
            o.str(w.payload)?;
        }
        if let Some(u) = c.username {
            o.str(u.as_bytes())?;
        }
        if let Some(p) = c.password {
            o.str(p)?;
        }
        Ok(())
    })
}

/// QoS 0 publish.
pub fn publish(buf: &mut [u8], topic: &str, payload: &[u8], retain: bool) -> Result<usize, Error> {
    packet(buf, 0x30 | u8::from(retain), |o| {
        o.str(topic.as_bytes())?;
        o.put(payload)
    })
}

/// Subscribe to `filters` with QoS 0.
pub fn subscribe(buf: &mut [u8], packet_id: u16, filters: &[&str]) -> Result<usize, Error> {
    packet(buf, 0x82, |o| {
        o.u16(packet_id)?;
        for f in filters {
            o.str(f.as_bytes())?;
            o.put(&[0])?;
        }
        Ok(())
    })
}

pub const PINGREQ: [u8; 2] = [0xc0, 0x00];
pub const DISCONNECT: [u8; 2] = [0xe0, 0x00];

/// A packet received from the broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Incoming<'a> {
    /// Return code 0 = accepted.
    ConnAck {
        code: u8,
    },
    SubAck {
        packet_id: u16,
        /// Every requested subscription was granted (no 0x80 return code).
        granted: bool,
    },
    Publish {
        topic: &'a str,
        payload: &'a [u8],
        /// Stored by the broker and replayed on subscribe (not sent just now).
        retained: bool,
    },
    PingResp,
    /// Anything else (PUBACK etc.); ignored by QoS 0 clients.
    Other,
}

/// Header length and total length of the packet starting at `data[0]`, once its fixed header
/// is complete; lets a client skip a packet too large for its buffer.
pub fn packet_len(data: &[u8]) -> Result<Option<(usize, usize)>, Error> {
    let mut remaining = 0usize;
    let mut i = 1;
    loop {
        let Some(&b) = data.get(i) else {
            return Ok(None);
        };
        remaining |= usize::from(b & 0x7f) << (7 * (i - 1));
        i += 1;
        if b & 0x80 == 0 {
            return Ok(Some((i, i + remaining)));
        }
        if i > 4 {
            return Err(Error::Malformed);
        }
    }
}

/// Parses one packet from the start of `data`. `Ok(None)` = need more bytes; otherwise the
/// packet and the number of bytes it used. `Err(Malformed)` only for a broken remaining-length
/// field: the stream cannot be followed any more (reconnect).
pub fn parse(data: &[u8]) -> Result<Option<(Incoming<'_>, usize)>, Error> {
    let Some(&first) = data.first() else {
        return Ok(None);
    };
    let Some((i, end)) = packet_len(data)? else {
        return Ok(None);
    };
    let Some(body) = data.get(i..end) else {
        return Ok(None);
    };
    // A complete packet with a broken body (a short CONNACK, a topic that is not UTF-8 or runs
    // past the body) is passed on as `Other` with its length, so the caller skips it instead
    // of stalling on it; only a broken length field (the stream is lost) is an error.
    Ok(Some((incoming(first, body).unwrap_or(Incoming::Other), end)))
}

/// The packet in a complete `body`; `None` when the body is malformed.
fn incoming(first: u8, body: &[u8]) -> Option<Incoming<'_>> {
    let u16_at = |i: usize| Some(u16::from_be_bytes([*body.get(i)?, *body.get(i + 1)?]));
    Some(match first >> 4 {
        2 => Incoming::ConnAck { code: *body.get(1)? },
        9 => Incoming::SubAck {
            packet_id: u16_at(0)?,
            granted: body.get(2..).is_some_and(|codes| !codes.is_empty() && codes.iter().all(|&c| c < 0x80)),
        },
        13 => Incoming::PingResp,
        3 => {
            let tlen = usize::from(u16_at(0)?);
            let topic = core::str::from_utf8(body.get(2..2 + tlen)?).ok()?;
            // QoS 1/2 publishes carry a packet id; we subscribed with QoS 0, but skip it anyway.
            let start = 2 + tlen + if (first >> 1) & 3 > 0 { 2 } else { 0 };
            Incoming::Publish { topic, payload: body.get(start..)?, retained: first & 1 != 0 }
        }
        _ => Incoming::Other,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_with_will_and_credentials() {
        let mut buf = [0u8; 128];
        let n = connect(
            &mut buf,
            &Connect {
                client_id: "kwl",
                username: Some("u"),
                password: Some(b"p"),
                will: Some(Will { topic: "a/availability", payload: b"offline", retain: true }),
                keep_alive_s: 60,
            },
        )
        .unwrap();
        let expected: &[u8] = &[
            0x10, 46, // CONNECT, remaining length
            0, 4, b'M', b'Q', b'T', b'T', 4, 0xe6, 0, 60, // header, flags = user|pass|will retain|will|clean
            0, 3, b'k', b'w', b'l', //
            0, 14, b'a', b'/', b'a', b'v', b'a', b'i', b'l', b'a', b'b', b'i', b'l', b'i', b't', b'y', //
            0, 7, b'o', b'f', b'f', b'l', b'i', b'n', b'e', //
            0, 1, b'u', 0, 1, b'p',
        ];
        assert_eq!(&buf[..n], expected);
    }

    #[test]
    fn publish_and_long_remaining_length() {
        let mut buf = [0u8; 2048];
        let n = publish(&mut buf, "t", b"hi", true).unwrap();
        assert_eq!(&buf[..n], &[0x31, 5, 0, 1, b't', b'h', b'i']);
        // 1200-byte discovery payload: two-byte remaining length.
        let payload = [b'x'; 1200];
        let n = publish(&mut buf, "homeassistant/fan/kwl/fan/config", &payload, true).unwrap();
        let remaining = 2 + 32 + 1200;
        assert_eq!(&buf[..3], &[0x31, (remaining % 128) as u8 | 0x80, (remaining / 128) as u8]);
        assert_eq!(n, 3 + remaining);
        assert_eq!(&buf[n - 3..n], b"xxx");
        assert_eq!(publish(&mut buf[..100], "t", &payload, false), Err(Error::BufferFull));
    }

    #[test]
    fn subscribe_packet() {
        let mut buf = [0u8; 64];
        let n = subscribe(&mut buf, 1, &["a/#", "b"]).unwrap();
        assert_eq!(&buf[..n], &[0x82, 12, 0, 1, 0, 3, b'a', b'/', b'#', 0, 0, 1, b'b', 0]);
    }

    #[test]
    fn parses_broker_packets() {
        assert_eq!(parse(&[0x20, 2, 0, 0]), Ok(Some((Incoming::ConnAck { code: 0 }, 4))));
        assert_eq!(parse(&[0x90, 3, 0, 7, 0]), Ok(Some((Incoming::SubAck { packet_id: 7, granted: true }, 5))));
        assert_eq!(parse(&[0x90, 4, 0, 7, 0, 0x80]), Ok(Some((Incoming::SubAck { packet_id: 7, granted: false }, 6))));
        assert_eq!(parse(&[0xd0, 0]), Ok(Some((Incoming::PingResp, 2))));
        let p = [0x30, 9, 0, 3, b'a', b'/', b'b', b'P', b'R', b'E', b'S'];
        assert_eq!(parse(&p), Ok(Some((Incoming::Publish { topic: "a/b", payload: b"PRES", retained: false }, 11))));
        let mut r = p;
        r[0] = 0x31;
        assert_eq!(parse(&r), Ok(Some((Incoming::Publish { topic: "a/b", payload: b"PRES", retained: true }, 11))));
        // QoS 1 publish has a packet id after the topic.
        let p = [0x32, 7, 0, 1, b't', 0, 9, b'O', b'N'];
        assert_eq!(parse(&p), Ok(Some((Incoming::Publish { topic: "t", payload: b"ON", retained: false }, 9))));
        assert_eq!(parse(&p[..5]), Ok(None), "incomplete");
        assert_eq!(parse(&[]), Ok(None));
        assert_eq!(parse(&[0x30, 0xff, 0xff, 0xff, 0xff, 1]), Err(Error::Malformed));
    }

    #[test]
    fn malformed_complete_packets_are_skipped() {
        // Each is complete but broken: returned as `Other` with its length, so the client
        // drops it and still sees the PINGRESP behind it.
        let broken: [&[u8]; 6] = [
            &[0x20, 0],                         // CONNACK without its two bytes
            &[0x20, 1, 0],                      // CONNACK cut short
            &[0x90, 1, 0],                      // SUBACK without a full packet id
            &[0x30, 1, 0],                      // PUBLISH without a full topic length
            &[0x30, 4, 0, 9, b'a', b'b'],       // topic length past the body
            &[0x30, 5, 0, 2, 0xff, 0xfe, b'x'], // topic not UTF-8
        ];
        for p in broken {
            let mut data = p.to_vec();
            data.extend_from_slice(&[0xd0, 0]);
            assert_eq!(parse(&data), Ok(Some((Incoming::Other, p.len()))), "{p:02x?}");
            assert_eq!(parse(&data[p.len()..]), Ok(Some((Incoming::PingResp, 2))));
        }
        // QoS 1 publish whose packet id runs past the body.
        assert_eq!(parse(&[0x32, 3, 0, 1, b't']), Ok(Some((Incoming::Other, 5))));
    }

    #[test]
    fn round_trip_publish() {
        let mut buf = [0u8; 64];
        let n = publish(&mut buf, "openkwl/fan/set", b"ON", false).unwrap();
        assert_eq!(parse(&buf[..n]), Ok(Some((Incoming::Publish { topic: "openkwl/fan/set", payload: b"ON", retained: false }, n))));
        // A packet larger than any buffer: its length is known from the header alone.
        assert_eq!(packet_len(&[0x30, 0xeb, 0x84, 0x01]), Ok(Some((4, 4 + 17_003))));
    }
}
