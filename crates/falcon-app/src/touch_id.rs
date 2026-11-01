//! Touch ID for the app lock, after Eagle's `privacy.enableTouchID`: on a
//! Mac that can use it, Preferences ▸ Password offers a switch (turning it
//! on asks for a fingerprint first), and the lock screen then shows a
//! Touch ID button beside the password field. The password always works.
//!
//! It goes through LocalAuthentication's `LAContext` (the
//! biometrics-only policy, as Electron's `promptTouchID` uses) with
//! `msg_send!`. The answer comes on a private queue; [`poll`] hands it to
//! the frame that asked, by ticket.

/// Whether this Mac can use Touch ID now (a sensor, and a finger
/// enrolled). Checked at most every few seconds, as a lid can close.
pub fn available() -> bool {
    imp::available()
}

/// Asks for a fingerprint with `reason` ("falcon is trying to <reason>").
/// Returns a ticket for [`poll`], or `None` when Touch ID can't be used.
/// A new request replaces one still waiting.
pub fn start(reason: &str, ctx: &eframe::egui::Context) -> Option<u64> {
    imp::start(reason, ctx)
}

/// The answer to request `ticket` once it is in: true when the
/// fingerprint matched, false when it failed or was cancelled.
pub fn poll(ticket: u64) -> Option<bool> {
    imp::poll(ticket)
}

#[cfg(target_os = "macos")]
mod imp {
    use std::cell::{Cell, RefCell};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2::msg_send;
    use objc2_foundation::NSString;

    #[link(name = "LocalAuthentication", kind = "framework")]
    unsafe extern "C" {}

    /// `LAPolicyDeviceOwnerAuthenticationWithBiometrics`.
    const BIOMETRICS: isize = 1;
    /// How long an availability check is reused.
    const RECHECK: Duration = Duration::from_secs(5);

    struct Pending {
        ticket: u64,
        /// Kept alive until the answer comes.
        _context: Retained<AnyObject>,
        answer: Arc<Mutex<Option<bool>>>,
    }

    thread_local! {
        static CHECKED: Cell<Option<(Instant, bool)>> = const { Cell::new(None) };
        static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };
        static NEXT: Cell<u64> = const { Cell::new(1) };
    }

    fn context() -> Option<Retained<AnyObject>> {
        let class = AnyClass::get(c"LAContext")?;
        // SAFETY: +new on an NSObject subclass returns a retained instance.
        let ctx: Option<Retained<AnyObject>> = unsafe { msg_send![class, new] };
        ctx
    }

    fn can_evaluate(ctx: &AnyObject) -> bool {
        // SAFETY: canEvaluatePolicy:error: takes an LAPolicy (NSInteger)
        // and an optional NSError** out parameter, and returns a BOOL.
        let ok: Bool = unsafe { msg_send![ctx, canEvaluatePolicy: BIOMETRICS, error: std::ptr::null_mut::<*mut AnyObject>()] };
        ok.as_bool()
    }

    pub fn available() -> bool {
        if let Some((at, ok)) = CHECKED.get()
            && at.elapsed() < RECHECK
        {
            return ok;
        }
        let ok = context().is_some_and(|c| can_evaluate(&c));
        CHECKED.set(Some((Instant::now(), ok)));
        ok
    }

    pub fn start(reason: &str, egui_ctx: &eframe::egui::Context) -> Option<u64> {
        let la = context()?;
        if !can_evaluate(&la) {
            return None;
        }
        let ticket = NEXT.get();
        NEXT.set(ticket + 1);
        let answer = Arc::new(Mutex::new(None));
        let (slot, repaint) = (answer.clone(), egui_ctx.clone());
        let reply = RcBlock::new(move |ok: Bool, _error: *mut AnyObject| {
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(ok.as_bool());
            repaint.request_repaint();
        });
        let reason = NSString::from_str(reason);
        // SAFETY: evaluatePolicy:localizedReason:reply: takes an LAPolicy,
        // an NSString and a block (BOOL, NSError*) -> void, which it copies
        // and calls once on a private queue; the block only touches
        // thread-safe values.
        unsafe {
            let _: () = msg_send![&*la, evaluatePolicy: BIOMETRICS, localizedReason: &*reason, reply: &*reply];
        }
        PENDING.with(|p| *p.borrow_mut() = Some(Pending { ticket, _context: la, answer }));
        Some(ticket)
    }

    pub fn poll(ticket: u64) -> Option<bool> {
        PENDING.with(|p| {
            let mut p = p.borrow_mut();
            let pending = p.as_ref().filter(|q| q.ticket == ticket)?;
            let got = (*pending.answer.lock().unwrap_or_else(|e| e.into_inner()))?;
            *p = None;
            Some(got)
        })
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn available() -> bool {
        false
    }
    pub fn start(_reason: &str, _ctx: &eframe::egui::Context) -> Option<u64> {
        None
    }
    pub fn poll(_ticket: u64) -> Option<bool> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_tickets_have_no_answer() {
        // Whatever the Mac, nothing was asked under this ticket.
        assert_eq!(poll(u64::MAX), None);
        // The check is cached: asking twice gives the same answer.
        assert_eq!(available(), available());
    }
}
