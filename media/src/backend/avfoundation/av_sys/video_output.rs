use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_av_foundation::AVPlayerItemVideoOutput;
use objc2_core_media::CMTime;
use objc2_core_video::{
    CVPixelBuffer, kCVPixelBufferPixelFormatTypeKey,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString};

pub(crate) struct AvVideoOutput {
    inner: Retained<AVPlayerItemVideoOutput>,
    /// The thread that created the output; every access asserts it.
    owner: std::thread::ThreadId,
}

impl AvVideoOutput {
    pub(crate) fn new() -> Self {
        // Build pixel-buffer attributes requesting bi-planar NV12 in video
        // range, the format hardware decoders produce: a pool request that
        // matches the decoder's output lets the video output hand over the
        // decoded buffer instead of transferring it into a BGRA one.
        // kCVPixelBufferPixelFormatTypeKey is &'static CFString.
        // SAFETY: CFString and NSString are toll-free bridged.
        // SAFETY: `kCVPixelBufferPixelFormatTypeKey` is a `&'static CFString`
        // toll-free bridged to `NSString`; the reference borrows the static.
        let key: &NSString =
            unsafe { &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString) };
        let value = NSNumber::new_u32(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);
        // SAFETY: dictionaryWithObject:forKey: expects key to conform to
        // NSCopying, which NSString does.  NSNumber conforms to AnyObject.
        // We cast through *const void to change the type parameter.
        // SAFETY: `dictionaryWithObject:forKey:` copies its arguments (the
        // key conforms to `NSCopying`) and returns a retained dictionary; the
        // cast to `NSDictionary<NSString, AnyObject>` keeps the runtime
        // layout (the generics are phantom) so the `Retained` is released
        // exactly once.
        let attrs: Retained<NSDictionary<NSString, AnyObject>> = unsafe {
            let dict = NSDictionary::<NSString, NSNumber>::dictionaryWithObject_forKey(
                &*value,
                ProtocolObject::from_ref(key as &NSString),
            );
            // Coerce value type via pointer cast (same layout at runtime).
            Retained::from_raw(Retained::into_raw(dict) as *mut NSDictionary<NSString, AnyObject>)
                .unwrap()
        };

        // SAFETY: `alloc`/`init...` is the two-step initializer the framework
        // requires; `attrs` is a valid attributes dictionary.
        let inner = unsafe {
            AVPlayerItemVideoOutput::initWithPixelBufferAttributes(
                AVPlayerItemVideoOutput::alloc(),
                Some(&*attrs),
            )
        };
        Self {
            inner,
            owner: std::thread::current().id(),
        }
    }

    /// The output, after checking that this is the thread that owns it.
    pub(crate) fn inner(&self) -> &Retained<AVPlayerItemVideoOutput> {
        assert_eq!(
            std::thread::current().id(),
            self.owner,
            "AVPlayerItemVideoOutput used from a thread other than the one that created it"
        );
        &self.inner
    }

    pub(crate) fn suppress_rendering(&self) {
        // SAFETY: `inner()` proved this is the creating thread and the
        // receiver is a retained ObjC instance; the arguments are framework types.
        unsafe { self.inner().setSuppressesPlayerRendering(true) };
    }

    pub(crate) fn item_time_for_host_time(&self, host_secs: f64) -> CMTime {
        // SAFETY: `inner()` proved this is the creating thread and the
        // receiver is a retained ObjC instance; the arguments are framework types.
        unsafe { self.inner().itemTimeForHostTime(host_secs) }
    }

    pub(crate) fn has_new_pixel_buffer(&self, time: CMTime) -> bool {
        // SAFETY: `inner()` proved this is the creating thread and the
        // receiver is a retained ObjC instance; the arguments are framework types.
        unsafe { self.inner().hasNewPixelBufferForItemTime(time) }
    }

    pub(crate) fn copy_pixel_buffer(&self, time: CMTime) -> Option<Retained<CVPixelBuffer>> {
        // SAFETY: `inner()` proved this is the creating thread and the
        // receiver is a retained ObjC instance; a null out-parameter asks the
        // framework for the buffer as the method's return value.
        unsafe {
            self.inner()
                .copyPixelBufferForItemTime_itemTimeForDisplay(time, std::ptr::null_mut())
        }
    }
}
