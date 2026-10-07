//! Per-operation output and cancellation, independent of either frontend.
use std::{
    cell::RefCell,
    io::Write,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
        Arc,
    },
};

#[derive(Debug)]
pub enum Event {
    Output(String),
    Diagnostic(String),
    Log(String),
    Phase(String),
    FlowSaved(std::path::PathBuf),
}
#[derive(Clone)]
pub struct Observer {
    pub events: Sender<Event>,
    pub cancelled: Arc<AtomicBool>,
}
thread_local! { static OBSERVER: RefCell<Option<Observer>> = const { RefCell::new(None) }; }
pub fn observe<T>(observer: Observer, operation: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            OBSERVER.with(|slot| slot.replace(None));
        }
    }
    OBSERVER.with(|slot| slot.replace(Some(observer)));
    let _reset = Reset;
    operation()
}
pub fn emit(event: Event) {
    OBSERVER.with(|slot| {
        if let Some(observer) = slot.borrow().as_ref() {
            let _ = observer.events.send(event);
        } else {
            match event {
                Event::Output(text) => {
                    let _ = std::io::stdout().lock().write_all(text.as_bytes());
                }
                Event::Diagnostic(text) => {
                    let _ = std::io::stderr().lock().write_all(text.as_bytes());
                }
                Event::Phase(_) | Event::Log(_) | Event::FlowSaved(_) => {}
            }
        }
    });
}
pub fn phase(text: &str) {
    emit(Event::Phase(text.to_owned()));
}
pub fn cancelled() -> bool {
    OBSERVER.with(|s| {
        s.borrow()
            .as_ref()
            .is_some_and(|o| o.cancelled.load(Ordering::Relaxed))
    })
}
pub fn checkpoint() -> anyhow::Result<()> {
    anyhow::ensure!(!cancelled(), "Operation cancelled");
    Ok(())
}
#[macro_export]
macro_rules! out { ($($arg:tt)*) => { $crate::runtime::emit($crate::runtime::Event::Output(format!("{}\n", format_args!($($arg)*)))) }; }
#[macro_export]
macro_rules! out_raw { ($($arg:tt)*) => { $crate::runtime::emit($crate::runtime::Event::Output(format!($($arg)*))) }; }
#[macro_export]
macro_rules! err { ($($arg:tt)*) => { $crate::runtime::emit($crate::runtime::Event::Diagnostic(format!("{}\n", format_args!($($arg)*)))) }; }
