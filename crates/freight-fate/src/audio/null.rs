//! Last resort: every primitive is a no-op (`_NullBackend`).
//!
//! The facade falls here when `FREIGHT_FATE_AUDIO_BACKEND` asks for a
//! backend the Rust build does not carry. Game logic never has to check: the volume settings still take,
//! radio and playlist files report the same refusals the Python null backend
//! raised, and everything else is silent.

use std::any::Any;

use super::backend::{AudioBackend, Buses};

#[derive(Debug, Clone, Default)]
pub struct NullBackend {
    buses: Buses,
}

impl NullBackend {
    pub fn new() -> Self {
        Self {
            buses: Buses::new(),
        }
    }
}

impl AudioBackend for NullBackend {
    fn name(&self) -> &'static str {
        "none"
    }

    fn enabled(&self) -> bool {
        false
    }

    fn buses(&self) -> &Buses {
        &self.buses
    }

    fn buses_mut(&mut self) -> &mut Buses {
        &mut self.buses
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
