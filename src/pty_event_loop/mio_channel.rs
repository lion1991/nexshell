//! mio-friendly mpsc channel.
//!
//! Vendored verbatim (with module path adjustments) from Warp:
//! `warp/app/src/terminal/local_tty/mio_channel.rs`. The same trick the
//! Alacritty event loop uses — wake a `mio::Poll` whenever a message hits the
//! mpsc queue, so the PTY thread can wait for either PTY readiness or new
//! input on the same `Poll::poll()` call.

pub use std::sync::mpsc::SendError;
use std::{
    io,
    sync::{mpsc, Arc, Mutex},
};

use mio::{event, Token, Waker};

/// Create a [`Sender`] and [`Receiver`] pair, for sending messages into a
/// [`mio`]-managed event loop.
pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let (tx, rx) = mpsc::channel();

    let state = Arc::new(Mutex::new(State {
        waker: None,
        needs_wake_on_register: false,
    }));

    (
        Sender {
            state: state.clone(),
            tx,
        },
        Receiver { state, rx },
    )
}

pub struct Receiver<T> {
    state: Arc<Mutex<State>>,
    rx: mpsc::Receiver<T>,
}

impl<T> Receiver<T> {
    pub fn try_recv(&self) -> Result<T, mpsc::TryRecvError> {
        self.rx.try_recv()
    }

    #[cfg(windows)]
    pub fn recv(&self) -> Result<T, mpsc::RecvError> {
        self.rx.recv()
    }
}

impl<T> event::Source for Receiver<T> {
    fn register(
        &mut self,
        registry: &mio::Registry,
        token: Token,
        _: mio::Interest,
    ) -> io::Result<()> {
        let mut state = self.state.lock().unwrap();

        if state.waker.is_none() {
            let waker = Waker::new(registry, token)?;
            if state.needs_wake_on_register {
                waker.wake()?;
                state.needs_wake_on_register = false;
            }
            state.waker = Some(waker);
        }

        Ok(())
    }

    fn reregister(
        &mut self,
        _registry: &mio::Registry,
        _token: Token,
        _: mio::Interest,
    ) -> io::Result<()> {
        Ok(())
    }

    fn deregister(&mut self, _: &mio::Registry) -> io::Result<()> {
        Ok(())
    }
}

pub struct Sender<T> {
    state: Arc<Mutex<State>>,
    tx: mpsc::Sender<T>,
}

impl<T> Sender<T> {
    pub fn send(&self, t: T) -> Result<(), SendError<T>> {
        self.tx.send(t)?;

        let mut state = self.state.lock().unwrap();
        if let Some(waker) = &mut state.waker {
            let _ = waker.wake();
        } else {
            state.needs_wake_on_register = true;
        }

        Ok(())
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            tx: self.tx.clone(),
        }
    }
}

struct State {
    waker: Option<Waker>,
    needs_wake_on_register: bool,
}
