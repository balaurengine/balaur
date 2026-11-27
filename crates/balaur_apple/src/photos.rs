//! `device.pick_photo` on Apple: iOS's `PHPickerViewController`, which needs
//! no permission to read the photo library, asked for a JPEG so a HEIC photo
//! arrives as one `image::encode` reads; a Mac's file dialog, the platform
//! crate's own.

/// Whether a picker is here to show.
pub(crate) fn available() -> bool {
    if cfg!(target_os = "ios") {
        objc2::runtime::AnyClass::get(c"PHPickerViewController").is_some()
    } else {
        true
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn pick(request: u64, report: &std::sync::mpsc::Sender<balaur_platform::PlatformEvent>) {
    balaur_platform::photos::pick_from_files(request, report);
}

#[cfg(target_os = "ios")]
pub(crate) use ios::pick;

#[cfg(target_os = "ios")]
mod ios {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::sync::mpsc::Sender;

    use balaur_platform::PlatformEvent;
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, NSObject, NSObjectProtocol};
    use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
    use objc2_foundation::{NSData, NSError, NSString};

    #[link(name = "PhotosUI", kind = "framework")]
    unsafe extern "C" {}

    /// The uniform type the picker is asked to hand a photo over as.
    const JPEG_TYPE: &str = "public.jpeg";
    const JPEG: &str = "image/jpeg";

    fn failed(request: u64, message: &str, report: &Sender<PlatformEvent>) {
        let _ = report.send(PlatformEvent::Failed {
            request,
            message: message.to_string(),
        });
    }

    /// Show the picker for one photo over the game.
    pub(crate) fn pick(request: u64, report: &Sender<PlatformEvent>) {
        let (Some(configuration), Some(controller), Some(filter), Some(mtm)) = (
            AnyClass::get(c"PHPickerConfiguration"),
            AnyClass::get(c"PHPickerViewController"),
            AnyClass::get(c"PHPickerFilter"),
            MainThreadMarker::new(),
        ) else {
            failed(request, "no photo picker on this system", report);
            return;
        };
        // Safety: PhotosUI's documented initialisers and setters, by message,
        // on the main thread the marker proved this is.
        let picker: Option<Retained<AnyObject>> = unsafe {
            let allocated: *mut AnyObject = msg_send![configuration, alloc];
            let configuration: *mut AnyObject = msg_send![allocated, init];
            let configuration = Retained::from_raw(configuration);
            configuration.and_then(|configuration| {
                let _: () = msg_send![&configuration, setSelectionLimit: 1isize];
                let images: *mut AnyObject = msg_send![filter, imagesFilter];
                let _: () = msg_send![&configuration, setFilter: images];
                let allocated: *mut AnyObject = msg_send![controller, alloc];
                let built: *mut AnyObject =
                    msg_send![allocated, initWithConfiguration: &*configuration];
                Retained::from_raw(built)
            })
        };
        let Some(picker) = picker else {
            failed(request, "the photo picker would not open", report);
            return;
        };
        let done = Rc::new(Cell::new(false));
        let delegate = Finished::new(
            mtm,
            Asked {
                request,
                report: report.clone(),
                done: Rc::clone(&done),
            },
        );
        // Safety: the delegate is held below for as long as the picker is up.
        unsafe {
            let _: () = msg_send![&picker, setDelegate: &*delegate];
        }
        if !crate::ui::present(Retained::as_ptr(&picker).cast_mut()) {
            failed(request, "no window to show the photo picker over", report);
            return;
        }
        HELD.with_borrow_mut(|held| {
            held.retain(|(_, _, answered)| !answered.get());
            held.push((picker, delegate, done));
        });
    }

    /// Read the picked photo as a JPEG, off the main thread, and answer.
    fn load(provider: *mut AnyObject, request: u64, report: Sender<PlatformEvent>) {
        let answer = report.clone();
        let loaded = RcBlock::new(move |data: *mut NSData, error: *mut NSError| {
            // Safety: the provider hands over a valid NSData or NSError, or nil.
            let event = match unsafe { (data.as_ref(), error.as_ref()) } {
                (Some(data), _) => PlatformEvent::Picked {
                    request,
                    bytes: data.to_vec(),
                    media_type: JPEG.to_string(),
                },
                (None, Some(error)) => PlatformEvent::Failed {
                    request,
                    message: error.localizedDescription().to_string(),
                },
                (None, None) => PlatformEvent::Failed {
                    request,
                    message: "the photo could not be read".to_string(),
                },
            };
            balaur_core::replay::report(&answer, event);
        });
        let kind = NSString::from_str(JPEG_TYPE);
        // Safety: an NSItemProvider the picker handed over, which copies the block.
        let progress: *mut AnyObject = unsafe {
            msg_send![provider, loadDataRepresentationForTypeIdentifier: &*kind, completionHandler: &*loaded]
        };
        if progress.is_null() {
            failed(request, "the photo could not be read", &report);
        }
    }

    /// What one open picker answers with.
    struct Asked {
        request: u64,
        report: Sender<PlatformEvent>,
        done: Rc<Cell<bool>>,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements, and this class
        // does not implement Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "BalaurPhotoPicked"]
        #[ivars = Asked]
        struct Finished;

        unsafe impl NSObjectProtocol for Finished {}

        impl Finished {
            /// `PHPickerViewControllerDelegate`, declared informally: the
            /// framework only sends the selector. No result is the picker
            /// shut with nothing picked.
            #[unsafe(method(picker:didFinishPicking:))]
            fn finished(&self, picker: *mut AnyObject, results: *mut AnyObject) {
                let Asked { request, report, done } = self.ivars();
                done.set(true);
                // Safety: the picker and the NSArray of results it hands over.
                unsafe {
                    let _: () = msg_send![
                        picker,
                        dismissViewControllerAnimated: true,
                        completion: Option::<&block2::DynBlock<dyn Fn()>>::None,
                    ];
                    let count: usize = if results.is_null() { 0 } else { msg_send![results, count] };
                    if count == 0 {
                        let _ = report.send(PlatformEvent::Cancelled { request: *request });
                        return;
                    }
                    let first: *mut AnyObject = msg_send![results, objectAtIndex: 0usize];
                    let provider: *mut AnyObject = msg_send![first, itemProvider];
                    if provider.is_null() {
                        failed(*request, "the photo could not be read", report);
                        return;
                    }
                    load(provider, *request, report.clone());
                }
            }
        }
    );

    impl Finished {
        fn new(mtm: MainThreadMarker, asked: Asked) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(asked);
            // Safety: NSObject's designated initialiser.
            unsafe { msg_send![super(this), init] }
        }
    }

    /// An open picker, its delegate, and whether it has answered.
    type Open = (Retained<AnyObject>, Retained<Finished>, Rc<Cell<bool>>);

    // The picker holds its delegate weakly, so both live here until the next
    // pick clears the ones that answered: dropping one from inside its own
    // callback would free the object running it.
    thread_local! {
        static HELD: RefCell<Vec<Open>> = const { RefCell::new(Vec::new()) };
    }
}
