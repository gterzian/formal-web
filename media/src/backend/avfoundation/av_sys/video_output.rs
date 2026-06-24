use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_av_foundation::AVPlayerItemVideoOutput;
use objc2_core_media::CMTime;
use objc2_core_video::{
    CVPixelBuffer, kCVPixelBufferPixelFormatTypeKey, kCVPixelFormatType_32BGRA,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString};

pub(crate) struct AvVideoOutput {
    pub(crate) inner: Retained<AVPlayerItemVideoOutput>,
}

impl AvVideoOutput {
    pub(crate) fn new() -> Self {
        // Build pixel-buffer attributes requesting 32BGRA format.
        // kCVPixelBufferPixelFormatTypeKey is &'static CFString.
        // SAFETY: CFString and NSString are toll-free bridged.
        let key: &NSString =
            unsafe { &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString) };
        let value = NSNumber::new_u32(kCVPixelFormatType_32BGRA);
        // SAFETY: dictionaryWithObject:forKey: expects key to conform to
        // NSCopying, which NSString does.  NSNumber conforms to AnyObject.
        // We cast through *const void to change the type parameter.
        let attrs: Retained<NSDictionary<NSString, AnyObject>> = unsafe {
            let dict = NSDictionary::<NSString, NSNumber>::dictionaryWithObject_forKey(
                &*value,
                ProtocolObject::from_ref(key as &NSString),
            );
            // Coerce value type via pointer cast (same layout at runtime).
            Retained::from_raw(Retained::into_raw(dict) as *mut NSDictionary<NSString, AnyObject>)
                .unwrap()
        };

        let inner = unsafe {
            AVPlayerItemVideoOutput::initWithPixelBufferAttributes(
                AVPlayerItemVideoOutput::alloc(),
                Some(&*attrs),
            )
        };
        Self { inner }
    }

    pub(crate) fn suppress_rendering(&self) {
        unsafe { self.inner.setSuppressesPlayerRendering(true) };
    }

    pub(crate) fn item_time_for_host_time(&self, host_secs: f64) -> CMTime {
        unsafe { self.inner.itemTimeForHostTime(host_secs) }
    }

    pub(crate) fn has_new_pixel_buffer(&self, time: CMTime) -> bool {
        unsafe { self.inner.hasNewPixelBufferForItemTime(time) }
    }

    pub(crate) fn copy_pixel_buffer(&self, time: CMTime) -> Option<Retained<CVPixelBuffer>> {
        unsafe {
            self.inner
                .copyPixelBufferForItemTime_itemTimeForDisplay(time, std::ptr::null_mut())
        }
    }
}
