//! `GET /system`, `POST /system/reboot`.

use serde::{Deserialize, Serialize};

use crate::{config::Ipv4String, time::DateTime};

/// Response of `GET /system`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemInfo {
    /// Semantic version of openKWL.
    pub firmware_version: heapless::String<16>,
    /// Build identifier, e.g. a git revision.
    pub build: heapless::String<40>,
    /// MCU part as detected (DBGMCU_IDCODE / flash size), e.g. `STM32F207 1024K`.
    pub mcu: heapless::String<24>,
    /// `aa:bb:cc:dd:ee:ff`
    pub mac: heapless::String<17>,
    /// Current address, `null` without a link or lease.
    pub ip: Option<Ipv4String>,
    /// Seconds since boot.
    pub uptime_s: u32,
    /// Cause of the last reset.
    pub reset_reason: ResetReason,
    /// SD card mounted.
    pub sd_card: bool,
    /// MQTT session to the broker is up.
    pub mqtt_connected: bool,
    /// Local time of the last successful SNTP sync, `null` if none since boot.
    pub last_time_sync: Option<DateTime>,
    /// Factory identity, as on the stock "Device data" page (`tinfo.htm`).
    pub device: DeviceIdentity,
}

/// Body of `POST /system/time`: sets the clock, e.g. from the browser when SNTP is off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetTimeRequest {
    /// Seconds since 1970-01-01 UTC.
    pub unix_s: u64,
}

impl crate::Validate for SetTimeRequest {
    fn validate(&self) -> Result<(), crate::FieldError> {
        // 2024-01-01 … 2099-12-31
        crate::check((openkwl_core::clock::MIN_VALID_UNIX_S..4_102_444_800).contains(&self.unix_s), "unix_s", "out of range")
    }
}

/// Factory identity of the unit. Read-only; taken over from the stock settings by the import
/// (`[device]` in `CONFIG.INI`). Fields are empty when unknown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceIdentity {
    /// Stock `v00000`, e.g. `KWL EC 300W R`.
    pub model: heapless::String<24>,
    /// Stock `v00001` "Ref. no." (parameter file), e.g. `04232001.hGP`.
    pub article: heapless::String<16>,
    /// Stock `v00303`.
    pub serial: heapless::String<12>,
    /// Stock `v00304`.
    pub production_code: heapless::String<12>,
    /// Stock `v00343`, the Helios portal pairing code. Not used by this firmware.
    pub security_code: heapless::String<12>,
}

/// Upper bound of the encoded [`SystemInfo`].
pub const SYSTEM_JSON_MAX: usize = 512;

/// From the RCC reset flags.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResetReason {
    /// Power-on/power-down reset.
    PowerOn,
    /// NRST pin.
    Pin,
    /// Software reset, e.g. `POST /system/reboot`.
    Software,
    /// Independent or window watchdog.
    Watchdog,
    /// Brown-out reset.
    Brownout,
    /// No flag set.
    #[default]
    Unknown,
}

// ---------------------------------------------------------------------------------------
// GET /system/details

/// Response of `GET /system/details`: everything the firmware knows about itself and its
/// surroundings, for the advanced view of the device page and for bug reports.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemDetails {
    /// The running image.
    pub firmware: FirmwareDetails,
    /// The microcontroller.
    pub chip: ChipDetails,
    /// Flash and RAM use.
    pub memory: MemoryDetails,
    /// Ethernet and IP.
    pub network: NetworkDetails,
    /// Clock and time synchronisation.
    pub time: TimeDetails,
    /// Home Assistant connection.
    pub mqtt: MqttDetails,
    /// SD card and the write budget.
    pub storage: StorageDetails,
    /// What the stock firmware left on the unit (for a rollback).
    pub stock: StockDetails,
    /// Counters since boot.
    pub counters: Counters,
    /// Raw board readings that confirm the pin assumptions on the bench.
    pub board: BoardDetails,
    /// Extension modules on the module bus (the KWL-KNX).
    pub modules: ModuleDetails,
}

/// Upper bound of the encoded [`SystemDetails`].
pub const SYSTEM_DETAILS_JSON_MAX: usize = 2506;

/// Supply of the module bus (+24 V on the RJ-10 socket for extension modules).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusPower {
    /// Not switched on (yet), or no module bus on this platform.
    #[default]
    Off,
    /// Soft start running.
    Starting,
    /// Supplying the modules.
    On,
    /// Switched off after repeated overcurrent (stock "Fehler 05").
    Overcurrent,
}

/// What the KWL-KNX module does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnxState {
    /// Not answered yet since start-up.
    #[default]
    Searching,
    /// Answers its polls.
    Connected,
    /// Did not answer (none fitted, or not powered); still asked every 10 s.
    NotFound,
}

/// Extension modules on the RS-485 module bus.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleDetails {
    /// The module bus supply.
    pub bus_power: BusPower,
    /// The KWL-KNX module.
    pub knx: KnxState,
    /// Firmware version the KNX module reports, e.g. `"1.20"`; `null` before it answered.
    pub knx_firmware: Option<heapless::String<12>>,
    /// Answers received from the module since boot.
    pub knx_frames: u32,
    /// Answers with a wrong checksum, address or length, and module error replies.
    pub knx_errors: u32,
    /// Polls without an answer.
    pub knx_timeouts: u32,
    /// Bytes received on the module bus since boot, usable or not.
    pub knx_rx_bytes: u32,
    /// Bytes received with a framing, noise or overrun error (wrong speed or format, or
    /// interference).
    pub knx_line_errors: u32,
    /// Current sense of the module socket (PC2) in mV; above 1000 for a second = overcurrent.
    pub bus_sense_mv: u32,
}


/// Raw board readings for bring-up: what the firmware found instead of assuming it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardDetails {
    /// The card-detect pin (PH13) said "card present" at boot; `null` when not read.
    pub card_detect_present: Option<bool>,
    /// The SD card switched on with PA15 low (the stock behaviour) rather than high; `null`
    /// without a card.
    pub sd_power_active_low: Option<bool>,
    /// Mains zero-cross pulses per second on PB3 (50 or 100 on 50 Hz mains, 0 on a bench
    /// supply); `null` before the first second.
    pub mains_pulses_per_s: Option<u16>,
    /// Management address of the Ethernet PHY found by the scan, `null` if none answered.
    pub phy_address: Option<u8>,
    /// Supply of the NTC pull-ups as measured on PC3 (smoothed), in mV; `null` before the
    /// first reading.
    pub ntc_supply_mv: Option<u16>,
    /// Smoothed 12-bit ADC values of T1 outdoor, T2 supply, T3 extract and T4 exhaust air.
    pub ntc_raw: Option<[u16; 4]>,
}

/// The running image.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirmwareDetails {
    /// Semantic version of openKWL.
    pub version: heapless::String<16>,
    /// Build identifier, e.g. a git revision.
    pub build: heapless::String<40>,
    /// `stm32f2` on the unit, `simulator` on a computer.
    pub platform: heapless::String<16>,
    /// Rust target triple, e.g. `thumbv7m-none-eabi`.
    pub target: heapless::String<24>,
    /// Compiler version, e.g. `1.95.0`.
    pub rustc: heapless::String<24>,
    /// `release` or `debug`.
    pub profile: heapless::String<8>,
    /// Build time, seconds since 1970-01-01 UTC; `null` when unknown.
    pub built_unix_s: Option<u64>,
    /// Seconds since boot.
    pub uptime_s: u32,
}

/// The microcontroller.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChipDetails {
    /// Part as detected, e.g. `STM32F207 1024K`.
    pub mcu: heapless::String<24>,
    /// `DBGMCU_IDCODE.DEV_ID`, e.g. `0x411` for STM32F2; 0 when unknown.
    pub device_id: u16,
    /// `DBGMCU_IDCODE.REV_ID`, e.g. `0x2000` (revision B).
    pub revision_id: u16,
    /// 96-bit unique device ID as 24 hex digits.
    pub unique_id: heapless::String<24>,
    /// Flash size in KiB (from the flash size register).
    pub flash_kb: u16,
    /// System clock in Hz.
    pub sysclk_hz: u32,
    /// External crystal frequency in Hz, `null` when running from the internal oscillator.
    pub hse_hz: Option<u32>,
    /// Cause of the last reset.
    pub reset_reason: ResetReason,
    /// Raw reset flags (`RCC_CSR` bits 24–31).
    pub reset_flags: u8,
}

/// Flash and RAM use.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryDetails {
    /// Size of the application slot behind the stock bootloader.
    pub flash_slot_bytes: u32,
    /// Size of the running image.
    pub image_bytes: u32,
    /// Total RAM.
    pub ram_bytes: u32,
    /// Statically allocated RAM (`.data` + `.bss`).
    pub static_ram_bytes: u32,
    /// RAM reserved for the stack.
    pub stack_bytes: u32,
    /// Most stack used since boot, `null` when not measured.
    pub stack_peak_bytes: Option<u32>,
}

/// Ethernet and IP.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkDetails {
    /// Ethernet link is up.
    pub link_up: bool,
    /// Address from DHCP (otherwise static).
    pub dhcp: bool,
    /// Name announced via DHCP and mDNS.
    pub hostname: heapless::String<32>,
    /// `aa:bb:cc:dd:ee:ff`
    pub mac: heapless::String<17>,
    /// Current address, `null` without a link or lease.
    pub ip: Option<Ipv4String>,
    /// Subnet prefix length, e.g. 24.
    pub prefix_len: Option<u8>,
    /// Default gateway.
    pub gateway: Option<Ipv4String>,
    /// DNS servers in use.
    pub dns: heapless::Vec<Ipv4String, 3>,
    /// Time server announced by DHCP (option 42), `null` if none. Always `null` on the unit:
    /// embassy-net's DHCP client does not pass the option on.
    pub ntp_server: Option<Ipv4String>,
    /// Answers `<hostname>.local` via mDNS.
    pub mdns: bool,
    /// Ethernet PHY as detected, e.g. `LAN8720`.
    pub phy: heapless::String<16>,
}

/// Where the current time came from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeSource {
    /// Not set since the battery-backed clock was lost.
    #[default]
    None,
    /// The battery-backed real-time clock kept it across a restart.
    Rtc,
    /// Set by hand (`POST /system/time`).
    Manual,
    /// Synchronised with a time server.
    Sntp,
}

/// Clock and time synchronisation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeDetails {
    /// Where the current time came from.
    pub source: TimeSource,
    /// Time server used for the last attempt (name or address), empty if none.
    pub server: heapless::String<64>,
    /// Local time of the last successful sync, `null` if none since boot.
    pub last_sync: Option<DateTime>,
    /// Clock correction applied by the last sync, in milliseconds.
    pub last_offset_ms: Option<i32>,
    /// Successful syncs since boot.
    pub syncs: u32,
    /// Clock source of the real-time clock: `LSE` (32 kHz crystal), `LSI` or `host`.
    pub rtc_clock: heapless::String<8>,
}

/// Home Assistant connection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MqttDetails {
    /// MQTT is switched on in the settings.
    pub enabled: bool,
    /// Session to the broker is up.
    pub connected: bool,
    /// Broker host name or address.
    pub broker: heapless::String<64>,
    /// Successful connections since boot.
    pub connects: u32,
    /// Messages sent since boot.
    pub published: u32,
    /// Commands received since boot.
    pub received: u32,
}

/// SD card and the write budget.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageDetails {
    /// Card mounted.
    pub present: bool,
    /// Card size in MB, `null` when unknown.
    pub capacity_mb: Option<u32>,
    /// Files written since boot (settings and state).
    pub writes: u32,
    /// Local time the operating state was last saved.
    pub last_state_save: Option<DateTime>,
    /// `CONFIG.INI` loaded without errors at boot.
    pub config_valid: bool,
}

/// What the stock firmware left on the unit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StockDetails {
    /// Version of the stock bootloader, e.g. `1.10`; empty when unknown.
    pub bootloader: heapless::String<16>,
    /// `FwBasis` in the card's `settings.ini`: the image the bootloader installs.
    pub fw_basis: heapless::String<16>,
}

/// Counters since boot.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counters {
    /// API requests handled.
    pub http_requests: u32,
    /// API requests answered with an error.
    pub http_errors: u32,
    /// Rejected login attempts.
    pub failed_logins: u32,
    /// Sessions currently open (normal and remembered).
    pub sessions: u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::round_trip;

    trait RepeatInto {
        fn repeat_into<const N: usize>(&self) -> heapless::String<N>;
    }
    impl RepeatInto for str {
        fn repeat_into<const N: usize>(&self) -> heapless::String<N> {
            let mut s = heapless::String::new();
            while s.push_str(self).is_ok() {}
            s
        }
    }

    #[test]
    fn set_time() {
        use crate::Validate;
        assert!(SetTimeRequest { unix_s: 1_792_186_862 }.validate().is_ok());
        assert!(SetTimeRequest { unix_s: 0 }.validate().is_err());
        round_trip::<_, 40>(&SetTimeRequest { unix_s: u64::MAX });
    }

    #[test]
    fn system_info_round_trip() {
        let s = SystemInfo {
            firmware_version: "0.1.0".try_into().unwrap(),
            build: "0123456789abcdef0123456789abcdef01234567".try_into().unwrap(),
            mcu: "STM32F207VGT6".try_into().unwrap(),
            mac: "00:1a:2b:3c:4d:5e".try_into().unwrap(),
            ip: Some("255.255.255.255".try_into().unwrap()),
            uptime_s: u32::MAX,
            reset_reason: ResetReason::Watchdog,
            sd_card: true,
            mqtt_connected: false,
            last_time_sync: Some(DateTime::from(openkwl_core::clock::TimeZone::CET.to_local(1_792_186_862))),
            device: DeviceIdentity {
                model: "KWL EC 300W R".try_into().unwrap(),
                article: "04232001.hGP".try_into().unwrap(),
                serial: "1000".try_into().unwrap(),
                production_code: "01001".try_into().unwrap(),
                security_code: "0123ABCD".try_into().unwrap(),
            },
        };
        let json = round_trip::<_, SYSTEM_JSON_MAX>(&s);
        assert!(json.contains(r#""device":{"model":"KWL EC 300W R","article":"04232001.hGP","#));

        let full = |n: usize| -> heapless::String<24> { core::iter::repeat_n('x', n).collect() };
        let worst = SystemInfo {
            firmware_version: "x".repeat_into(),
            build: "x".repeat_into(),
            mcu: full(24),
            device: DeviceIdentity {
                model: full(24),
                article: "x".repeat_into(),
                serial: "x".repeat_into(),
                production_code: "x".repeat_into(),
                security_code: "x".repeat_into(),
            },
            ..s
        };
        round_trip::<_, SYSTEM_JSON_MAX>(&worst);
        assert!(json.contains(r#""reset_reason":"watchdog""#));
    }

    #[test]
    fn details_worst_case_fits() {
        let ip = || -> Ipv4String { "255.255.255.255".try_into().unwrap() };
        let when = Some(DateTime::from(openkwl_core::clock::TimeZone::CET.to_local(1_792_186_862)));
        let d = SystemDetails {
            firmware: FirmwareDetails {
                version: "x".repeat_into(),
                build: "x".repeat_into(),
                platform: "x".repeat_into(),
                target: "x".repeat_into(),
                rustc: "x".repeat_into(),
                profile: "x".repeat_into(),
                built_unix_s: Some(u64::MAX),
                uptime_s: u32::MAX,
            },
            chip: ChipDetails {
                mcu: "x".repeat_into(),
                device_id: u16::MAX,
                revision_id: u16::MAX,
                unique_id: "x".repeat_into(),
                flash_kb: u16::MAX,
                sysclk_hz: u32::MAX,
                hse_hz: Some(u32::MAX),
                reset_reason: ResetReason::Watchdog,
                reset_flags: u8::MAX,
            },
            memory: MemoryDetails {
                flash_slot_bytes: u32::MAX,
                image_bytes: u32::MAX,
                ram_bytes: u32::MAX,
                static_ram_bytes: u32::MAX,
                stack_bytes: u32::MAX,
                stack_peak_bytes: Some(u32::MAX),
            },
            network: NetworkDetails {
                link_up: true,
                dhcp: false,
                hostname: "x".repeat_into(),
                mac: "x".repeat_into(),
                ip: Some(ip()),
                prefix_len: Some(u8::MAX),
                gateway: Some(ip()),
                dns: heapless::Vec::from_slice(&[ip(), ip(), ip()]).unwrap(),
                ntp_server: Some(ip()),
                mdns: false,
                phy: "x".repeat_into(),
            },
            time: TimeDetails {
                source: TimeSource::Manual,
                // From `/config/time`, which allows control characters (six bytes escaped).
                server: "\u{1}".repeat_into(),
                last_sync: when,
                last_offset_ms: Some(i32::MIN),
                syncs: u32::MAX,
                rtc_clock: "x".repeat_into(),
            },
            mqtt: MqttDetails {
                enabled: false,
                connected: false,
                // From `/config/mqtt`: `"` passes its checks (two bytes escaped).
                broker: "\"".repeat_into(),
                connects: u32::MAX,
                published: u32::MAX,
                received: u32::MAX,
            },
            storage: StorageDetails {
                present: false,
                capacity_mb: Some(u32::MAX),
                writes: u32::MAX,
                last_state_save: when,
                config_valid: false,
            },
            stock: StockDetails { bootloader: "x".repeat_into(), fw_basis: "x".repeat_into() },
            counters: Counters { http_requests: u32::MAX, http_errors: u32::MAX, failed_logins: u32::MAX, sessions: u8::MAX },
            board: BoardDetails {
                card_detect_present: Some(false),
                sd_power_active_low: Some(false),
                mains_pulses_per_s: Some(u16::MAX),
                phy_address: Some(u8::MAX),
                ntc_supply_mv: Some(u16::MAX),
                ntc_raw: Some([u16::MAX; 4]),
            },
            modules: ModuleDetails {
                bus_power: BusPower::Overcurrent,
                knx: KnxState::NotFound,
                knx_firmware: Some("x".repeat_into()),
                knx_frames: u32::MAX,
                knx_errors: u32::MAX,
                knx_timeouts: u32::MAX,
                knx_rx_bytes: u32::MAX,
                knx_line_errors: u32::MAX,
                bus_sense_mv: u32::MAX,
            },
        };
        let json = round_trip::<_, SYSTEM_DETAILS_JSON_MAX>(&d);
        assert!(json.contains(r#""source":"manual""#));
    }
}
