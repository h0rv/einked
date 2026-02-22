//! Activity lifecycle, navigation transitions, and fixed-depth stack.

#[cfg(feature = "alloc")]
extern crate alloc;

use heapless::Vec;

use crate::core::{Rect, Theme};
use crate::input::{InputEvent, InputSource};
use crate::refresh::RefreshHint;
use crate::storage::{FileStore, SettingsStore};

pub trait Ui<T: Theme> {
    fn clear(&mut self, _theme: &T) {}
    fn label(&mut self, _text: &str) {}
    fn paragraph(&mut self, text: &str) {
        self.label(text);
    }
    fn divider(&mut self) {}
    fn status_bar(&mut self, _left: &str, _right: &str) {}
    fn set_refresh_hint(&mut self, _hint: RefreshHint) {}
}

impl<T: Theme> Ui<T> for () {}

/// Shared app context passed to lifecycle and input handlers.
pub struct Context<'a, T: Theme> {
    pub theme: &'a T,
    pub screen: Rect,
    pub settings: &'a mut dyn SettingsStore,
    pub files: &'a mut dyn FileStore,
}

#[cfg(not(feature = "alloc"))]
pub type ActivityId = u8;

#[cfg(not(feature = "alloc"))]
pub trait ActivityFactory<T: Theme> {
    fn get_mut(&mut self, id: ActivityId) -> Option<&mut dyn Activity<T>>;
}

#[cfg(feature = "alloc")]
pub enum Transition<T: Theme> {
    Stay,
    Push(alloc::boxed::Box<dyn Activity<T>>),
    Pop,
    Replace(alloc::boxed::Box<dyn Activity<T>>),
    Reset(alloc::boxed::Box<dyn Activity<T>>),
}

#[cfg(not(feature = "alloc"))]
pub enum Transition<T: Theme> {
    Stay,
    Push(ActivityId),
    Pop,
    Replace(ActivityId),
    Reset(ActivityId),
    _Phantom(core::marker::PhantomData<T>),
}

pub trait Activity<T: Theme> {
    fn on_enter(&mut self, _ctx: &mut Context<'_, T>) {}
    fn on_pause(&mut self, _ctx: &mut Context<'_, T>) {}
    fn on_resume(&mut self, _ctx: &mut Context<'_, T>) {}
    fn on_exit(&mut self, _ctx: &mut Context<'_, T>) {}
    fn on_input(&mut self, event: InputEvent, ctx: &mut Context<'_, T>) -> Transition<T>;
    fn render(&self, ui: &mut dyn Ui<T>);
    fn refresh_hint(&self) -> RefreshHint {
        RefreshHint::Adaptive
    }
}

#[cfg(feature = "alloc")]
pub struct ActivityStack<T: Theme, const DEPTH: usize> {
    stack: Vec<alloc::boxed::Box<dyn Activity<T>>, DEPTH>,
}

#[cfg(not(feature = "alloc"))]
pub struct ActivityStack<T: Theme, const DEPTH: usize> {
    stack: Vec<ActivityId, DEPTH>,
    _theme: core::marker::PhantomData<T>,
}

#[cfg(feature = "alloc")]
impl<T: Theme, const DEPTH: usize> ActivityStack<T, DEPTH> {
    pub const fn new() -> Self {
        Self { stack: Vec::new() }
    }

    pub fn push_root(
        &mut self,
        mut root: alloc::boxed::Box<dyn Activity<T>>,
        ctx: &mut Context<'_, T>,
    ) -> Result<(), alloc::boxed::Box<dyn Activity<T>>> {
        root.on_enter(ctx);
        self.stack.push(root)
    }

    pub fn tick(
        &mut self,
        input: Option<InputEvent>,
        ui: &mut dyn Ui<T>,
        ctx: &mut Context<'_, T>,
    ) -> bool {
        let Some(top) = self.stack.last_mut() else {
            return false;
        };

        let transition = if let Some(event) = input {
            top.on_input(event, ctx)
        } else {
            Transition::Stay
        };

        self.apply_transition(transition, ctx);
        if let Some(current) = self.stack.last() {
            ui.set_refresh_hint(current.refresh_hint());
            current.render(ui);
            true
        } else {
            false
        }
    }

    pub fn tick_source(
        &mut self,
        input: &mut impl InputSource,
        ui: &mut dyn Ui<T>,
        ctx: &mut Context<'_, T>,
    ) -> bool {
        self.tick(input.poll(), ui, ctx)
    }

    fn apply_transition(&mut self, transition: Transition<T>, ctx: &mut Context<'_, T>) {
        match transition {
            Transition::Stay => {}
            Transition::Pop => {
                if let Some(mut top) = self.stack.pop() {
                    top.on_exit(ctx);
                }
                if let Some(next) = self.stack.last_mut() {
                    next.on_resume(ctx);
                }
            }
            Transition::Push(mut next) => {
                if let Some(current) = self.stack.last_mut() {
                    current.on_pause(ctx);
                }
                next.on_enter(ctx);
                let _ = self.stack.push(next);
            }
            Transition::Replace(mut next) => {
                if let Some(mut old) = self.stack.pop() {
                    old.on_exit(ctx);
                }
                next.on_enter(ctx);
                let _ = self.stack.push(next);
            }
            Transition::Reset(mut root) => {
                while let Some(mut item) = self.stack.pop() {
                    item.on_exit(ctx);
                }
                root.on_enter(ctx);
                let _ = self.stack.push(root);
            }
        }
    }
}

#[cfg(not(feature = "alloc"))]
impl<T: Theme, const DEPTH: usize> ActivityStack<T, DEPTH> {
    pub const fn new() -> Self {
        Self {
            stack: Vec::new(),
            _theme: core::marker::PhantomData,
        }
    }

    pub fn push_root<F: ActivityFactory<T>>(
        &mut self,
        root: ActivityId,
        factory: &mut F,
        ctx: &mut Context<'_, T>,
    ) -> Result<(), ActivityId> {
        if self.stack.is_full() {
            return Err(root);
        }

        if let Some(activity) = factory.get_mut(root) {
            activity.on_enter(ctx);
            let _ = self.stack.push(root);
            Ok(())
        } else {
            Err(root)
        }
    }

    pub fn tick<F: ActivityFactory<T>>(
        &mut self,
        input: Option<InputEvent>,
        ui: &mut dyn Ui<T>,
        factory: &mut F,
        ctx: &mut Context<'_, T>,
    ) -> bool {
        let Some(top_id) = self.stack.last().copied() else {
            return false;
        };

        let transition = if let Some(event) = input {
            if let Some(top) = factory.get_mut(top_id) {
                top.on_input(event, ctx)
            } else {
                Transition::Pop
            }
        } else {
            Transition::Stay
        };

        self.apply_transition(transition, factory, ctx);

        let Some(current_id) = self.stack.last().copied() else {
            return false;
        };

        if let Some(current) = factory.get_mut(current_id) {
            ui.set_refresh_hint(current.refresh_hint());
            current.render(ui);
            true
        } else {
            false
        }
    }

    pub fn tick_source<F: ActivityFactory<T>>(
        &mut self,
        input: &mut impl InputSource,
        ui: &mut dyn Ui<T>,
        factory: &mut F,
        ctx: &mut Context<'_, T>,
    ) -> bool {
        self.tick(input.poll(), ui, factory, ctx)
    }

    fn apply_transition<F: ActivityFactory<T>>(
        &mut self,
        transition: Transition<T>,
        factory: &mut F,
        ctx: &mut Context<'_, T>,
    ) {
        match transition {
            Transition::Stay => {}
            Transition::Pop => {
                if let Some(top_id) = self.stack.pop()
                    && let Some(top) = factory.get_mut(top_id)
                {
                    top.on_exit(ctx);
                }
                if let Some(next_id) = self.stack.last().copied()
                    && let Some(next) = factory.get_mut(next_id)
                {
                    next.on_resume(ctx);
                }
            }
            Transition::Push(next_id) => {
                if self.stack.is_full() {
                    return;
                }

                let Some(current_id) = self.stack.last().copied() else {
                    return;
                };

                if let Some(current) = factory.get_mut(current_id) {
                    current.on_pause(ctx);
                }

                if let Some(next) = factory.get_mut(next_id) {
                    next.on_enter(ctx);
                    let _ = self.stack.push(next_id);
                } else if let Some(current) = factory.get_mut(current_id) {
                    // Roll back pause if factory cannot provide next activity.
                    current.on_resume(ctx);
                }
            }
            Transition::Replace(next_id) => {
                if self.stack.is_empty() {
                    return;
                }

                if factory.get_mut(next_id).is_none() {
                    return;
                }

                if let Some(old_id) = self.stack.pop()
                    && let Some(old) = factory.get_mut(old_id)
                {
                    old.on_exit(ctx);
                }

                if let Some(next) = factory.get_mut(next_id) {
                    next.on_enter(ctx);
                    let _ = self.stack.push(next_id);
                }
            }
            Transition::Reset(root_id) => {
                if factory.get_mut(root_id).is_none() {
                    return;
                }

                while let Some(item_id) = self.stack.pop() {
                    if let Some(item) = factory.get_mut(item_id) {
                        item.on_exit(ctx);
                    }
                }
                if let Some(root) = factory.get_mut(root_id) {
                    root.on_enter(ctx);
                    let _ = self.stack.push(root_id);
                }
            }
            Transition::_Phantom(_) => {}
        }
    }
}

#[cfg(feature = "alloc")]
impl<T: Theme, const DEPTH: usize> Default for ActivityStack<T, DEPTH> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(feature = "alloc"))]
impl<T: Theme, const DEPTH: usize> Default for ActivityStack<T, DEPTH> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(test, feature = "alloc"))]
mod tests {
    use alloc::boxed::Box;
    use core::cell::Cell;

    use super::*;
    use crate::core::DefaultTheme;
    use crate::storage::FileStoreError;

    struct DummySettings;
    impl SettingsStore for DummySettings {
        fn load_raw(&self, _key: u8, _buf: &mut [u8]) -> usize {
            0
        }
        fn save_raw(&mut self, _key: u8, _data: &[u8]) {}
    }

    struct DummyFiles;
    impl FileStore for DummyFiles {
        fn list(&self, _path: &str, _out: &mut dyn FnMut(&str)) {}
        fn read<'a>(&self, _path: &str, _buf: &'a mut [u8]) -> Result<&'a [u8], FileStoreError> {
            Ok(&[])
        }
        fn exists(&self, _path: &str) -> bool {
            false
        }
    }

    struct DummyUi;
    impl Ui<DefaultTheme> for DummyUi {}

    struct HintUi<'a> {
        hint: &'a Cell<RefreshHint>,
    }

    impl Ui<DefaultTheme> for HintUi<'_> {
        fn set_refresh_hint(&mut self, hint: RefreshHint) {
            self.hint.set(hint);
        }
    }

    struct DummyActivity {
        pops: bool,
    }

    impl Activity<DefaultTheme> for DummyActivity {
        fn on_input(
            &mut self,
            _event: InputEvent,
            _ctx: &mut Context<'_, DefaultTheme>,
        ) -> Transition<DefaultTheme> {
            if self.pops {
                Transition::Pop
            } else {
                Transition::Stay
            }
        }
        fn render(&self, _ui: &mut dyn Ui<DefaultTheme>) {}

        fn refresh_hint(&self) -> RefreshHint {
            if self.pops {
                RefreshHint::Full
            } else {
                RefreshHint::Partial
            }
        }
    }

    #[test]
    fn stack_pops_to_empty() {
        let mut stack: ActivityStack<DefaultTheme, 4> = ActivityStack::new();
        let theme = DefaultTheme;
        let mut settings = DummySettings;
        let mut files = DummyFiles;
        let mut ctx = Context {
            theme: &theme,
            screen: crate::core::Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            settings: &mut settings,
            files: &mut files,
        };
        assert!(
            stack
                .push_root(Box::new(DummyActivity { pops: true }), &mut ctx)
                .is_ok()
        );
        let mut ui = DummyUi;
        let alive = stack.tick(
            Some(InputEvent::Press(crate::input::Button::Confirm)),
            &mut ui,
            &mut ctx,
        );
        assert!(!alive);
    }

    #[test]
    fn stack_propagates_activity_refresh_hint() {
        let mut stack: ActivityStack<DefaultTheme, 4> = ActivityStack::new();
        let theme = DefaultTheme;
        let mut settings = DummySettings;
        let mut files = DummyFiles;
        let mut ctx = Context {
            theme: &theme,
            screen: crate::core::Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            settings: &mut settings,
            files: &mut files,
        };
        assert!(
            stack
                .push_root(Box::new(DummyActivity { pops: false }), &mut ctx)
                .is_ok()
        );

        let captured = Cell::new(RefreshHint::Adaptive);
        let mut ui = HintUi { hint: &captured };
        let alive = stack.tick(None, &mut ui, &mut ctx);
        assert!(alive);
        assert_eq!(captured.get(), RefreshHint::Partial);
    }
}

#[cfg(all(test, not(feature = "alloc")))]
mod no_alloc_tests {
    use super::*;
    use crate::core::DefaultTheme;
    use crate::storage::FileStoreError;

    struct DummySettings;
    impl SettingsStore for DummySettings {
        fn load_raw(&self, _key: u8, _buf: &mut [u8]) -> usize {
            0
        }
        fn save_raw(&mut self, _key: u8, _data: &[u8]) {}
    }

    struct DummyFiles;
    impl FileStore for DummyFiles {
        fn list(&self, _path: &str, _out: &mut dyn FnMut(&str)) {}
        fn read<'a>(&self, _path: &str, _buf: &'a mut [u8]) -> Result<&'a [u8], FileStoreError> {
            Ok(&[])
        }
        fn exists(&self, _path: &str) -> bool {
            false
        }
    }

    struct DummyUi;
    impl Ui<DefaultTheme> for DummyUi {}

    #[derive(Default)]
    struct Trace {
        enter: u8,
        pause: u8,
        resume: u8,
        exit: u8,
    }

    struct TestActivity {
        transition: Transition<DefaultTheme>,
        trace: Trace,
    }

    impl TestActivity {
        fn new(transition: Transition<DefaultTheme>) -> Self {
            Self {
                transition,
                trace: Trace::default(),
            }
        }
    }

    impl Activity<DefaultTheme> for TestActivity {
        fn on_enter(&mut self, _ctx: &mut Context<'_, DefaultTheme>) {
            self.trace.enter += 1;
        }
        fn on_pause(&mut self, _ctx: &mut Context<'_, DefaultTheme>) {
            self.trace.pause += 1;
        }
        fn on_resume(&mut self, _ctx: &mut Context<'_, DefaultTheme>) {
            self.trace.resume += 1;
        }
        fn on_exit(&mut self, _ctx: &mut Context<'_, DefaultTheme>) {
            self.trace.exit += 1;
        }
        fn on_input(
            &mut self,
            _event: InputEvent,
            _ctx: &mut Context<'_, DefaultTheme>,
        ) -> Transition<DefaultTheme> {
            core::mem::replace(&mut self.transition, Transition::Stay)
        }
        fn render(&self, _ui: &mut dyn Ui<DefaultTheme>) {}
    }

    struct Pool {
        activities: [Option<TestActivity>; 3],
    }

    impl Pool {
        fn new(
            a0: Transition<DefaultTheme>,
            a1: Transition<DefaultTheme>,
            a2: Transition<DefaultTheme>,
        ) -> Self {
            Self {
                activities: [
                    Some(TestActivity::new(a0)),
                    Some(TestActivity::new(a1)),
                    Some(TestActivity::new(a2)),
                ],
            }
        }
    }

    impl ActivityFactory<DefaultTheme> for Pool {
        fn get_mut(&mut self, id: ActivityId) -> Option<&mut dyn Activity<DefaultTheme>> {
            self.activities
                .get_mut(usize::from(id))
                .and_then(Option::as_mut)
                .map(|activity| activity as &mut dyn Activity<DefaultTheme>)
        }
    }

    #[test]
    fn push_replace_reset_use_static_pool() {
        let mut stack: ActivityStack<DefaultTheme, 4> = ActivityStack::new();
        let theme = DefaultTheme;
        let mut settings = DummySettings;
        let mut files = DummyFiles;
        let mut ctx = Context {
            theme: &theme,
            screen: crate::core::Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            settings: &mut settings,
            files: &mut files,
        };
        let mut ui = DummyUi;

        let mut pool = Pool::new(
            Transition::Push(1),
            Transition::Replace(2),
            Transition::Reset(0),
        );

        assert!(stack.push_root(0, &mut pool, &mut ctx).is_ok());

        assert!(stack.tick(
            Some(InputEvent::Press(crate::input::Button::Confirm)),
            &mut ui,
            &mut pool,
            &mut ctx
        ));
        assert!(stack.tick(
            Some(InputEvent::Press(crate::input::Button::Confirm)),
            &mut ui,
            &mut pool,
            &mut ctx
        ));
        assert!(stack.tick(
            Some(InputEvent::Press(crate::input::Button::Confirm)),
            &mut ui,
            &mut pool,
            &mut ctx
        ));

        let a0 = pool.activities[0].as_ref().expect("activity 0 present");
        let a1 = pool.activities[1].as_ref().expect("activity 1 present");
        let a2 = pool.activities[2].as_ref().expect("activity 2 present");

        assert_eq!(a0.trace.enter, 2);
        assert_eq!(a0.trace.pause, 1);
        assert_eq!(a0.trace.resume, 0);
        assert_eq!(a0.trace.exit, 1);

        assert_eq!(a1.trace.enter, 1);
        assert_eq!(a1.trace.exit, 1);

        assert_eq!(a2.trace.enter, 1);
        assert_eq!(a2.trace.exit, 1);
    }

    #[test]
    fn missing_activity_id_does_not_kill_stack() {
        let mut stack: ActivityStack<DefaultTheme, 4> = ActivityStack::new();
        let theme = DefaultTheme;
        let mut settings = DummySettings;
        let mut files = DummyFiles;
        let mut ctx = Context {
            theme: &theme,
            screen: crate::core::Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            settings: &mut settings,
            files: &mut files,
        };
        let mut ui = DummyUi;

        let mut pool = Pool::new(Transition::Push(9), Transition::Stay, Transition::Stay);
        assert!(stack.push_root(0, &mut pool, &mut ctx).is_ok());

        let alive = stack.tick(
            Some(InputEvent::Press(crate::input::Button::Confirm)),
            &mut ui,
            &mut pool,
            &mut ctx,
        );

        assert!(alive);
        let a0 = pool.activities[0].as_ref().expect("activity 0 present");
        assert_eq!(a0.trace.pause, 1);
        assert_eq!(a0.trace.resume, 1);
    }
}
