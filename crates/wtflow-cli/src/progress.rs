//! Terminal-only activity feedback. Document output always stays on stdout.
use std::{
    io::{self, IsTerminal, Write},
    sync::mpsc::{self, RecvTimeoutError, Sender},
    thread::{self, JoinHandle},
    time::Duration,
};

pub fn enabled(disabled: bool) -> bool {
    !disabled && io::stderr().is_terminal() && std::env::var("TERM").as_deref() != Ok("dumb")
}

/// Stop and erase the spinner before emitting results or diagnostic messages.
pub struct Progress {
    worker: Option<(Sender<()>, JoinHandle<()>)>,
}

impl Progress {
    pub fn start(enabled: bool, message: &str) -> Self {
        crate::runtime::phase(message);
        if !enabled {
            return Self { worker: None };
        }
        let (stop, receiver) = mpsc::channel();
        let message = message.to_owned();
        draw('|', &message);
        let worker = thread::Builder::new()
            .name("wtflow-progress".into())
            .spawn(move || {
                let frames = ['/', '-', '\\', '|'];
                let mut frame = 0;
                while let Err(RecvTimeoutError::Timeout) =
                    receiver.recv_timeout(Duration::from_millis(100))
                {
                    draw(frames[frame % frames.len()], &message);
                    frame = (frame + 1) % frames.len();
                }
            });
        match worker {
            Ok(worker) => Self {
                worker: Some((stop, worker)),
            },
            Err(_) => {
                clear();
                Self { worker: None }
            }
        }
    }

    pub fn finish(&mut self) {
        if let Some((stop, worker)) = self.worker.take() {
            let _ = stop.send(());
            let _ = worker.join();
            clear();
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.finish();
    }
}

fn draw(frame: char, message: &str) {
    let mut stderr = io::stderr().lock();
    let _ = write!(stderr, "\r\x1b[2K{frame} {message}");
    let _ = stderr.flush();
}

fn clear() {
    let mut stderr = io::stderr().lock();
    let _ = write!(stderr, "\r\x1b[2K");
    let _ = stderr.flush();
}
