/// Receives piece-processing progress without coupling the core library to a UI.
pub trait ProgressReporter: Sync {
    fn begin(&self, total: usize);
    fn advance(&self, delta: usize);
    fn finish(&self);
    /// Clears progress after a failure or cancellation without reporting success.
    fn abandon(&self);
}

pub(crate) struct ProgressGuard<'a> {
    reporter: Option<&'a dyn ProgressReporter>,
    completed: bool,
}

impl<'a> ProgressGuard<'a> {
    pub(crate) fn begin(reporter: Option<&'a dyn ProgressReporter>, total: usize) -> Self {
        if let Some(reporter) = reporter {
            reporter.begin(total);
        }
        Self {
            reporter,
            completed: false,
        }
    }

    pub(crate) fn advance(&self, delta: usize) {
        if let Some(reporter) = self.reporter {
            reporter.advance(delta);
        }
    }

    pub(crate) fn finish(&mut self) {
        if self.completed {
            return;
        }
        self.completed = true;
        if let Some(reporter) = self.reporter {
            reporter.finish();
        }
    }
}

impl Drop for ProgressGuard<'_> {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        self.completed = true;
        if let Some(reporter) = self.reporter {
            reporter.abandon();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Recording {
        begins: AtomicUsize,
        advances: AtomicUsize,
        finishes: AtomicUsize,
        abandons: AtomicUsize,
    }

    impl ProgressReporter for Recording {
        fn begin(&self, _total: usize) {
            self.begins.fetch_add(1, Ordering::SeqCst);
        }

        fn advance(&self, delta: usize) {
            self.advances.fetch_add(delta, Ordering::SeqCst);
        }

        fn finish(&self) {
            self.finishes.fetch_add(1, Ordering::SeqCst);
        }

        fn abandon(&self) {
            self.abandons.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn guard_finishes_on_success() {
        let recording = Recording::default();
        {
            let mut guard = ProgressGuard::begin(Some(&recording), 3);
            guard.advance(2);
            guard.finish();
        }

        assert_eq!(recording.begins.load(Ordering::SeqCst), 1);
        assert_eq!(recording.advances.load(Ordering::SeqCst), 2);
        assert_eq!(recording.finishes.load(Ordering::SeqCst), 1);
        assert_eq!(recording.abandons.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn guard_abandons_when_dropped_without_finish() {
        let recording = Recording::default();
        {
            let guard = ProgressGuard::begin(Some(&recording), 3);
            guard.advance(1);
        }

        assert_eq!(recording.begins.load(Ordering::SeqCst), 1);
        assert_eq!(recording.advances.load(Ordering::SeqCst), 1);
        assert_eq!(recording.finishes.load(Ordering::SeqCst), 0);
        assert_eq!(recording.abandons.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn finish_is_idempotent() {
        let recording = Recording::default();
        {
            let mut guard = ProgressGuard::begin(Some(&recording), 1);
            guard.finish();
            guard.finish();
        }

        assert_eq!(recording.finishes.load(Ordering::SeqCst), 1);
        assert_eq!(recording.abandons.load(Ordering::SeqCst), 0);
    }
}
