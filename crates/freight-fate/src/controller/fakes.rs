//! Test doubles for the controller seam: a pad that records what was asked
//! of it, a subsystem that hands out such pads and counts how often it was
//! brought up or torn down (the Python tests' `object()` controller and
//! `FakeSDL`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::{PadDevice, PadSubsystem, PadSubsystemFactory};

/// What a [`FakePad`] was told to do.
#[derive(Debug, Default)]
pub struct FakePadLog {
    pub rumbles: Vec<(u16, u16, u32)>,
    pub stops: u32,
}

/// A bound pad with a fixed instance id.
pub struct FakePad {
    pub instance_id: u32,
    pub attached: Rc<Cell<bool>>,
    pub mapping: Option<String>,
    pub log: Rc<RefCell<FakePadLog>>,
}

impl FakePad {
    pub fn new(instance_id: u32) -> Self {
        Self {
            instance_id,
            attached: Rc::new(Cell::new(true)),
            mapping: None,
            log: Rc::new(RefCell::new(FakePadLog::default())),
        }
    }

    /// A handle the test keeps to flip `attached` or read the rumble log.
    pub fn handle(&self) -> FakePadHandle {
        FakePadHandle {
            attached: Rc::clone(&self.attached),
            log: Rc::clone(&self.log),
        }
    }
}

/// The test's view of a pad the manager now owns.
#[derive(Clone)]
pub struct FakePadHandle {
    pub attached: Rc<Cell<bool>>,
    pub log: Rc<RefCell<FakePadLog>>,
}

impl PadDevice for FakePad {
    fn instance_id(&self) -> u32 {
        self.instance_id
    }

    fn attached(&self) -> bool {
        self.attached.get()
    }

    fn rumble(&mut self, low: u16, high: u16, duration_ms: u32) {
        self.log.borrow_mut().rumbles.push((low, high, duration_ms));
    }

    fn stop_rumble(&mut self) {
        self.log.borrow_mut().stops += 1;
    }

    fn mapping(&self) -> Option<String> {
        self.mapping.clone()
    }
}

/// What a [`FakeSubsystem`] and its factory saw.
#[derive(Debug, Default)]
pub struct FakeSdlLog {
    /// How many times the factory brought a subsystem up (`init`).
    pub inits: u32,
    /// How many subsystems were dropped (`quit`).
    pub quits: u32,
    /// Every `open(index)` call.
    pub opens: Vec<u32>,
}

/// A subsystem with `pads` recognized controllers (name, instance id).
pub struct FakeSubsystem {
    pub pads: Vec<(String, u32)>,
    pub log: Rc<RefCell<FakeSdlLog>>,
}

impl PadSubsystem for FakeSubsystem {
    fn count(&self) -> u32 {
        self.pads.len() as u32
    }

    fn is_controller(&self, index: u32) -> bool {
        (index as usize) < self.pads.len()
    }

    fn name_for_index(&self, index: u32) -> Option<String> {
        self.pads.get(index as usize).map(|(name, _)| name.clone())
    }

    fn open(&self, index: u32) -> Option<Box<dyn PadDevice>> {
        self.log.borrow_mut().opens.push(index);
        let (_, instance_id) = self.pads.get(index as usize)?;
        Some(Box::new(FakePad::new(*instance_id)))
    }
}

impl Drop for FakeSubsystem {
    fn drop(&mut self) {
        self.log.borrow_mut().quits += 1;
    }
}

/// A factory handing out [`FakeSubsystem`]s with the given pads, recording
/// into the shared log.
pub fn fake_factory(pads: Vec<(String, u32)>, log: Rc<RefCell<FakeSdlLog>>) -> PadSubsystemFactory {
    Box::new(move || {
        log.borrow_mut().inits += 1;
        Some(Box::new(FakeSubsystem {
            pads: pads.clone(),
            log: Rc::clone(&log),
        }))
    })
}
