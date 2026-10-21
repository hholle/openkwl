//! Operating mode and the resulting ventilation level.
//!
//! Priority, highest first: a timed party/quiet program, an active holiday, then the base
//! mode (manual level or the weekly program).
//!
//! Choosing a level or manual mode leaves the weekly program. (The stock UI hides the level
//! buttons in auto mode, so there one first switches to manual.) Returning to auto:
//! - stock: every day at 00:00 if enabled ([`ModeState::midnight_reset`], `v02116`);
//! - new: after a configurable time in manual mode ([`ModeState::set_auto_return`]). Every
//!   manual change restarts it; a running party/quiet program is not cut short.

pub const MAX_LEVEL: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Manual,
    Auto,
    Party,
    Quiet,
    Holiday,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {
    Manual,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramKind {
    Party,
    Quiet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Program {
    pub kind: ProgramKind,
    pub level: u8,
    pub remaining_s: u32,
}

/// What the fans should do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effective {
    pub mode: Mode,
    pub level: u8,
    /// Seconds left in a party/quiet program.
    pub remaining_s: Option<u32>,
    /// Seconds until manual mode returns to auto, if the timer is enabled.
    pub auto_return_s: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeState {
    pub base: Base,
    pub manual_level: u8,
    pub program: Option<Program>,
    auto_return_after_s: Option<u32>,
    manual_elapsed_s: u32,
}

impl ModeState {
    pub const fn new(base: Base, manual_level: u8) -> Self {
        Self { base, manual_level, program: None, auto_return_after_s: None, manual_elapsed_s: 0 }
    }

    /// Return to auto after this long in manual mode; `None` disables the timer.
    pub fn set_auto_return(&mut self, after_s: Option<u32>) {
        self.auto_return_after_s = after_s.filter(|&s| s > 0);
    }

    /// Seconds until the timer returns to auto (0 = as soon as a running program ends).
    pub fn auto_return_remaining_s(&self) -> Option<u32> {
        match self.base {
            Base::Manual => self.auto_return_after_s.map(|limit| limit.saturating_sub(self.manual_elapsed_s)),
            Base::Auto => None,
        }
    }

    /// Sets a level. Changes the running program's level, otherwise switches to manual.
    pub fn set_level(&mut self, level: u8) {
        let level = level.min(MAX_LEVEL);
        match self.program.as_mut() {
            Some(p) => p.level = level,
            None => {
                self.base = Base::Manual;
                self.manual_level = level;
                self.manual_elapsed_s = 0;
            }
        }
    }

    /// Ends any program and returns to manual at the current manual level.
    pub fn set_manual(&mut self) {
        self.program = None;
        self.base = Base::Manual;
        self.manual_elapsed_s = 0;
    }

    /// Ends any program and follows the weekly program.
    pub fn set_auto(&mut self) {
        self.program = None;
        self.base = Base::Auto;
    }

    /// Ends a party/quiet program early; the base mode (manual level or auto) takes over.
    pub fn end_program(&mut self) {
        self.program = None;
    }

    pub fn start_program(&mut self, kind: ProgramKind, level: u8, duration_s: u32) {
        self.program = Some(Program { kind, level: level.min(MAX_LEVEL), remaining_s: duration_s });
    }

    /// Call once per second.
    pub fn tick_second(&mut self) {
        if let Some(p) = self.program.as_mut() {
            p.remaining_s = p.remaining_s.saturating_sub(1);
            if p.remaining_s == 0 {
                self.program = None;
            }
        }
        if self.base == Base::Manual {
            self.manual_elapsed_s = self.manual_elapsed_s.saturating_add(1);
            if self.program.is_none() && self.auto_return_remaining_s() == Some(0) {
                self.base = Base::Auto;
            }
        }
    }

    /// Stock option "activate Auto at 00:00": call at local midnight when enabled.
    pub fn midnight_reset(&mut self) {
        self.base = Base::Auto;
    }

    /// `scheduled`: weekly-program level (`None` if the clock is not set or no slot applies);
    /// `holiday`: level from an active holiday; `fallback`: level used by Auto without a schedule.
    pub fn resolve(&self, scheduled: Option<u8>, holiday: Option<u8>, fallback: u8) -> Effective {
        if let Some(p) = self.program {
            let mode = match p.kind {
                ProgramKind::Party => Mode::Party,
                ProgramKind::Quiet => Mode::Quiet,
            };
            let auto_return_s = self.auto_return_remaining_s();
            return Effective { mode, level: p.level, remaining_s: Some(p.remaining_s), auto_return_s };
        }
        if let Some(level) = holiday {
            return Effective { mode: Mode::Holiday, level, remaining_s: None, auto_return_s: None };
        }
        match self.base {
            Base::Manual => Effective {
                mode: Mode::Manual,
                level: self.manual_level,
                remaining_s: None,
                auto_return_s: self.auto_return_remaining_s(),
            },
            Base::Auto => {
                Effective { mode: Mode::Auto, level: scheduled.unwrap_or(fallback), remaining_s: None, auto_return_s: None }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priorities() {
        let mut m = ModeState::new(Base::Auto, 2);
        assert_eq!(m.resolve(Some(3), None, 1), Effective { mode: Mode::Auto, level: 3, remaining_s: None, auto_return_s: None });
        assert_eq!(m.resolve(None, None, 1).level, 1, "fallback without schedule");
        assert_eq!(m.resolve(Some(3), Some(0), 1).mode, Mode::Holiday);
        m.start_program(ProgramKind::Quiet, 0, 180 * 60);
        let e = m.resolve(Some(3), Some(1), 1);
        assert_eq!((e.mode, e.level, e.remaining_s), (Mode::Quiet, 0, Some(10_800)));
    }

    #[test]
    fn program_expires_and_returns() {
        let mut m = ModeState::new(Base::Manual, 2);
        m.start_program(ProgramKind::Party, 4, 3);
        m.tick_second();
        m.tick_second();
        assert_eq!(m.resolve(None, None, 1).mode, Mode::Party);
        m.tick_second();
        assert_eq!(m.resolve(None, None, 1), Effective { mode: Mode::Manual, level: 2, remaining_s: None, auto_return_s: None });
    }

    #[test]
    fn ending_a_program_returns_to_the_base() {
        let mut m = ModeState::new(Base::Manual, 2);
        m.start_program(ProgramKind::Party, 4, 600);
        m.end_program();
        assert_eq!(m.resolve(Some(1), None, 1), Effective { mode: Mode::Manual, level: 2, remaining_s: None, auto_return_s: None });
        m.set_auto();
        m.start_program(ProgramKind::Quiet, 0, 600);
        m.end_program();
        assert_eq!(m.resolve(Some(3), None, 1).mode, Mode::Auto);
    }

    #[test]
    fn setting_a_level() {
        let mut m = ModeState::new(Base::Auto, 2);
        m.set_level(4);
        assert_eq!(m.resolve(Some(1), None, 1), Effective { mode: Mode::Manual, level: 4, remaining_s: None, auto_return_s: None });
        m.start_program(ProgramKind::Party, 3, 60);
        m.set_level(9);
        assert_eq!(m.resolve(None, None, 1).level, 4, "clamped, applied to the program");
        assert_eq!(m.manual_level, 4, "manual level unchanged by program edits");
        m.set_auto();
        assert_eq!(m.resolve(Some(1), None, 1).mode, Mode::Auto);
        m.set_manual();
        m.midnight_reset();
        assert_eq!(m.base, Base::Auto);
    }

    #[test]
    fn auto_return_timer() {
        let mut m = ModeState::new(Base::Auto, 2);
        m.set_auto_return(Some(60));
        assert_eq!(m.resolve(Some(1), None, 1).auto_return_s, None, "not counting in auto");
        m.set_level(3);
        for _ in 0..30 {
            m.tick_second();
        }
        assert_eq!(m.resolve(Some(1), None, 1).auto_return_s, Some(30));
        m.set_level(4);
        assert_eq!(m.resolve(Some(1), None, 1).auto_return_s, Some(60), "a manual change restarts the timer");
        for _ in 0..60 {
            m.tick_second();
        }
        assert_eq!(m.resolve(Some(1), None, 1).mode, Mode::Auto);
    }

    #[test]
    fn auto_return_waits_for_a_running_program() {
        let mut m = ModeState::new(Base::Manual, 2);
        m.set_auto_return(Some(10));
        m.start_program(ProgramKind::Party, 4, 30);
        for _ in 0..20 {
            m.tick_second();
        }
        let e = m.resolve(Some(1), None, 1);
        assert_eq!((e.mode, e.auto_return_s), (Mode::Party, Some(0)), "due, but party continues");
        for _ in 0..10 {
            m.tick_second();
        }
        assert_eq!(m.resolve(Some(1), None, 1).mode, Mode::Auto, "returns when the program ends");
    }

    #[test]
    fn auto_return_disabled() {
        let mut m = ModeState::new(Base::Manual, 2);
        m.set_auto_return(Some(0));
        for _ in 0..100_000 {
            m.tick_second();
        }
        let e = m.resolve(Some(1), None, 1);
        assert_eq!((e.mode, e.auto_return_s), (Mode::Manual, None));
    }
}
