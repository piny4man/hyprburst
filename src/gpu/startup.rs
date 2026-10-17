//! Open-path work that runs on a worker thread while the window and GL
//! context are created on the main thread, joined only where its result is
//! first needed.

use std::thread::JoinHandle;

/// A value being computed on a worker thread. The first [`Pending::get`]
/// joins it; later calls return the stored result.
pub(crate) enum Pending<T> {
    Running(JoinHandle<Result<T, String>>),
    Done(Result<T, String>),
}

impl<T: Send + 'static> Pending<T> {
    /// Start `work` on a thread named `name`.
    pub(crate) fn spawn(
        name: &str,
        work: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> Self {
        match std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(work)
        {
            Ok(handle) => Self::Running(handle),
            Err(err) => Self::Done(Err(format!("cannot start {name} thread: {err}"))),
        }
    }

    /// The result, joining the worker the first time. A panic in the worker
    /// becomes an error.
    pub(crate) fn get(&mut self) -> Result<&mut T, String> {
        if let Self::Running(_) = self {
            let Self::Running(handle) = std::mem::replace(self, Self::Done(Err(String::new())))
            else {
                unreachable!()
            };
            let name = handle.thread().name().unwrap_or("worker").to_owned();
            *self = Self::Done(
                handle
                    .join()
                    .unwrap_or_else(|_| Err(format!("{name} thread panicked"))),
            );
        }
        match self {
            Self::Done(Ok(value)) => Ok(value),
            Self::Done(Err(err)) => Err(err.clone()),
            Self::Running(_) => unreachable!(),
        }
    }

    /// The result if it has already been joined.
    pub(crate) fn ready(&self) -> Option<&T> {
        match self {
            Self::Done(Ok(value)) => Some(value),
            _ => None,
        }
    }

    /// [`Pending::ready`], mutably.
    pub(crate) fn ready_mut(&mut self) -> Option<&mut T> {
        match self {
            Self::Done(Ok(value)) => Some(value),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn assert_send<T: Send>() {}

    #[test]
    fn open_path_values_can_move_to_a_worker() {
        assert_send::<crate::gpu::font::FontSet>();
        assert_send::<crate::domain::launcher_core::LauncherCore>();
    }

    #[test]
    fn work_runs_on_a_named_worker_thread() {
        let main = thread::current().id();
        let mut pending = Pending::spawn("hyprburst-test", move || {
            let me = thread::current();
            Ok((me.id() != main, me.name().map(str::to_owned)))
        });
        assert_eq!(
            pending.get().unwrap(),
            &mut (true, Some("hyprburst-test".to_owned()))
        );
    }

    #[test]
    fn get_joins_once_and_keeps_the_value() {
        let mut pending = Pending::spawn("hyprburst-test", || Ok(vec![1]));
        assert!(pending.ready().is_none(), "not joined yet");
        pending.get().unwrap().push(2);
        assert_eq!(pending.get().unwrap(), &mut vec![1, 2]);
        assert_eq!(pending.ready_mut(), Some(&mut vec![1, 2]));
        assert_eq!(pending.ready(), Some(&vec![1, 2]));
    }

    #[test]
    fn errors_and_panics_surface_as_errors() {
        let mut failed: Pending<()> = Pending::spawn("hyprburst-test", || Err("no font".into()));
        assert_eq!(failed.get().unwrap_err(), "no font");
        assert_eq!(failed.get().unwrap_err(), "no font");
        assert!(failed.ready_mut().is_none());

        let mut panicked: Pending<()> = Pending::spawn("hyprburst-test", || panic!("boom"));
        let err = panicked.get().unwrap_err();
        assert!(err.contains("hyprburst-test"), "{err}");
    }
}
