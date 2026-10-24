//! Reference frames, derived from the stock firmware's block encoders.

use super::*;

fn hex(s: &str) -> std::vec::Vec<u8> {
    s.split_whitespace().map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
}

extern crate std;

/// The values of the test vectors: level 2 in Auto, 2026-10-17 14:32:05 (Saturday), the live
/// status word, link up with DHCP, T1–T4 9.2/19.6/21.9/12.5 °C, Standard 2, filter 3 months
/// with 129 600 min left, the live fan minutes.
fn snapshot() -> Snapshot {
    Snapshot {
        level: 2,
        auto: true,
        fan_pct: 47,
        time: Some(DateTime { year: 2026, month: 10, day: 17, weekday: 6, hour: 14, minute: 32, second: 5 }),
        status: 0x8807_0081,
        errors: 0,
        warnings: 0,
        info: 0,
        network: 0x14,
        uptime_ms: 0x0012_3456,
        temperatures: [Some(9.2), Some(19.6), Some(21.9), Some(12.5)],
        programme: 1,
        filter_months: 3,
        filter_remaining_min: 129_600,
        supply_minutes: 6_233_791,
        extract_minutes: 6_245_858,
    }
}

fn write_frame(id: u16, s: &Snapshot) -> std::vec::Vec<u8> {
    let mut regs = [0u16; 35];
    let n = block_registers(id, s, &mut regs);
    let mut out = [0u8; FRAME_MAX];
    let len = modbus::write_multiple(&mut out, 0x14, 0, &regs[..n]).unwrap();
    out[..len].to_vec()
}

#[test]
fn poll_request() {
    let mut out = [0u8; FRAME_MAX];
    let n = Module { address: 0x14, ..Module::new() }.request(&mut out);
    assert_eq!(&out[..n], hex("14 04 00 00 00 10 F3 03"));
}

#[test]
fn block_1() {
    let expected = hex("14 10 00 00 00 19 32 00 01 00 01 00 2C
         2F 82 11 0A 00 1A 00 06 00 00 20 05 00 0E 00 00
         00 81 88 07 00 00 00 00 00 00 00 00 00 00 00 00
         00 00 00 00 14 22 00 00 34 56 00 12 E3 92");
    assert_eq!(write_frame(1, &snapshot()), expected);
}

#[test]
fn block_31_temperatures() {
    let expected = hex("14 10 00 00 00 15 2A 00 01 00 1F 00 24
         00 1F 00 00 33 33 41 13 CC CD 41 9C 33 33 41 AF 00 00 41 48
         3C 00 46 1C 3C 00 46 1C 3C 00 46 1C 3C 00 46 1C E6 29");
    assert_eq!(write_frame(31, &snapshot()), expected);
}

#[test]
fn blocks_50_15_53_56() {
    let s = snapshot();
    assert_eq!(write_frame(50, &s), hex("14 10 00 00 00 07 0E 00 01 00 32 00 08 00 32 03 00 FA 40 00 01 30 6E"));
    assert_eq!(write_frame(15, &s), hex("14 10 00 00 00 05 0A 00 01 00 0F 00 04 00 0F 00 01 ED FF"));
    assert_eq!(write_frame(56, &s), hex("14 10 00 00 00 05 0A 00 01 00 38 00 04 00 38 00 00 DB 32"));
    // Block 53 with the vector's heater minutes zeroed: this unit has no heaters.
    let mut regs = [0u16; 35];
    let n = block_registers(53, &s, &mut regs);
    assert_eq!(&regs[..n], &[1, 53, 0x1C, 0x0035, 0, 0x1EBF, 0x005F, 0x4DE2, 0x005F, 0, 0, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn unknown_block_is_empty() {
    let mut regs = [0u16; 35];
    assert_eq!(block_registers(0x28, &snapshot(), &mut regs), 3);
    assert_eq!(&regs[..3], &[1, 0x28, 0]);
}

#[test]
fn missing_values() {
    let s = Snapshot { temperatures: [None, Some(19.6), None, None], time: None, ..snapshot() };
    let mut regs = [0u16; 35];
    block_registers(31, &s, &mut regs);
    // T1 = 9999.0 = 00 3C 1C 46 → registers 0x3C00, 0x461C.
    assert_eq!(&regs[5..7], &[0x3C00, 0x461C]);
    block_registers(1, &s, &mut regs);
    assert_eq!(regs[4] >> 8, 0, "no clock: month 0");
}

fn answer(reg0: u16, reg1: u16, reg12: u16) -> std::vec::Vec<u8> {
    let regs = [reg0, reg1, 0, 0, 0x14, 3, 0, 0, 100, 0, 100, 0, reg12, 0, 0, 0];
    let mut f = std::vec![0x14, 0x04, 0x20];
    for r in regs {
        f.extend_from_slice(&r.to_be_bytes());
    }
    let c = modbus::crc(&f);
    f.extend_from_slice(&c.to_le_bytes());
    f
}

#[test]
fn answer_frames_match_the_vectors() {
    assert_eq!(
        answer(0, 0, 0x83),
        hex("14 04 20 00 00 00 00 00 00 00 00 00 14 00 03 00 00 00 00 00 64 00 00 00 64 00 00 00 83 00 00 00 00 00 00 2A A5")
    );
    assert_eq!(answer(1, 1, 0x83)[35..], hex("7B 64")[..]);
    assert_eq!(answer(2, 15, 0x83)[35..], hex("4B 6F")[..]);
}

#[test]
fn fan_input_follows_stock() {
    let mut f = FanInput::new();
    // First answer after start-up: ignored.
    assert_eq!(f.on_poll(0x14, 0x14, 0x03, false, 2), None);
    // Manual level 3 while manual at 2.
    assert_eq!(f.on_poll(0x14, 0x14, 0x03, false, 2), Some(FanCommand::Manual(3)));
    // Unchanged: nothing (the web UI may have changed the level meanwhile).
    assert_eq!(f.on_poll(0x14, 0x14, 0x03, false, 1), None);
    assert_eq!(f.on_poll(0x14, 0x14, 0x04, false, 1), Some(FanCommand::Manual(4)));
    // Auto from KNX; a level sent along is ignored in Auto.
    assert_eq!(f.on_poll(0x14, 0x14, 0x82, false, 4), Some(FanCommand::Auto));
    assert_eq!(f.on_poll(0x14, 0x14, 0x83, true, 1), None);
    // Another module address: ignored.
    assert_eq!(f.on_poll(0x14, 0x15, 0x01, true, 1), None);
    // Auto → manual 3 from KNX: mode and level in one poll.
    assert_eq!(f.on_poll(0x14, 0x14, 0x03, true, 1), Some(FanCommand::Manual(3)));
    // Levels above 4 are clamped (stock would keep the old level).
    assert_eq!(f.on_poll(0x14, 0x14, 0x09, false, 3), Some(FanCommand::Manual(4)));
}

#[test]
fn a_steady_manual_report_does_not_leave_auto() {
    // Stock starts with "last mode = manual": a module that keeps reporting manual changes
    // nothing while the unit runs in Auto; only a change of the automatic bit counts.
    let mut f = FanInput::new();
    assert_eq!(f.on_poll(0x14, 0x14, 0x03, true, 2), None);
    assert_eq!(f.on_poll(0x14, 0x14, 0x03, true, 2), None);
    assert_eq!(f.on_poll(0x14, 0x14, 0x04, true, 2), None, "a level in Auto is only remembered");
}

#[test]
fn first_auto_is_applied_on_the_second_poll() {
    // Stock compares the high nibble with 0 at first: a module reporting Auto switches to Auto.
    let mut f = FanInput::new();
    assert_eq!(f.on_poll(0x14, 0x14, 0x83, false, 2), None);
    assert_eq!(f.on_poll(0x14, 0x14, 0x83, false, 2), Some(FanCommand::Auto));
}

#[test]
fn transaction_with_block_request_and_push() {
    let s = snapshot();
    let mut m = Module { address: 0x14, ..Module::new() };
    let mut out = [0u8; FRAME_MAX];
    let (cmd, f) = m.on_answer(&answer(1, 1, 0x83), &s, &mut out).unwrap();
    assert_eq!(cmd, None);
    let Followup::Write { len, qty } = f else { panic!("{f:?}") };
    assert_eq!((len, qty), (59, 25));
    assert_eq!(m.presence, Presence::Connected);
    assert_eq!(m.firmware, 100);
    assert_eq!(m.on_followup_answer(&hex("14 10 00 00 00 19 03 06"), f), Ok(None));

    let (_, f) = m.on_answer(&answer(2, 15, 0x83), &s, &mut out).unwrap();
    assert_eq!(f, Followup::Read { len: 8, qty: 5 });
    assert_eq!(&out[..8], hex("14 03 00 00 00 05 87 0C"));
    let push = m.on_followup_answer(&hex("14 03 0A 00 02 00 0F 00 04 00 0F 00 03 71 81"), f);
    assert_eq!(push, Ok(Some(Push::Programme(3))));
    assert_eq!(m.on_followup_answer(&hex("14 10 00 00 00 18 03 06"), Followup::Write { len: 0, qty: 25 }), Err(Error::Crc));
}

#[test]
fn exception_and_timeouts() {
    let mut m = Module { address: 0x14, ..Module::new() };
    let mut out = [0u8; FRAME_MAX];
    assert_eq!(m.on_answer(&hex("14 84 02 D3 05"), &snapshot(), &mut out), Err(Error::Exception(2)));
    assert_eq!(m.presence, Presence::Searching, "an exception is no timeout");
    let delays: std::vec::Vec<u32> = (0..11)
        .map(|_| {
            m.on_timeout();
            m.next_poll_ms()
        })
        .collect();
    assert_eq!(delays, [1000, 1400, 2400, 3400, 3400, 4400, 4400, 4400, 4400, 4400, ABSENT_POLL_MS]);
    assert_eq!(m.presence, Presence::NotFound);
    // Unlike stock, a module that comes back is found again (here when 0x14 is asked).
    m.address = 0x14;
    m.on_answer(&answer(0, 0, 0), &snapshot(), &mut out).unwrap();
    assert_eq!((m.presence, m.next_poll_ms()), (Presence::Connected, POLL_MS));
}

#[test]
fn asks_both_addresses_until_one_answers() {
    let mut m = Module::new();
    let mut out = [0u8; FRAME_MAX];
    let n = m.request(&mut out);
    assert_eq!(out[0], 0x19, "{:02x?}", &out[..n]);
    m.on_timeout();
    m.request(&mut out);
    assert_eq!(out[0], 0x14);
    m.on_timeout();
    assert_eq!(m.address, 0x19);
    // Found at 0x14: stays there, also through a missed answer.
    m.address = 0x14;
    m.on_answer(&answer(0, 0, 0x83), &snapshot(), &mut out).unwrap();
    assert_eq!(m.presence, Presence::Connected);
    m.on_timeout();
    assert_eq!(m.address, 0x14);
}
