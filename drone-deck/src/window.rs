pub fn fit_size(work: (f32, f32)) -> (f32, f32) {
    (
        (work.0 - 64.).clamp(360., 1280.),
        (work.1 - 96.).clamp(320., 820.),
    )
}
pub fn startup_size() -> (f32, f32) {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct Rect {
            left: i32,
            top: i32,
            right: i32,
            bottom: i32,
        }
        #[link(name = "user32")]
        unsafe extern "system" {
            fn SystemParametersInfoW(
                action: u32,
                param: u32,
                value: *mut core::ffi::c_void,
                flags: u32,
            ) -> i32;
            fn GetDpiForSystem() -> u32;
        }
        let mut area = Rect {
            left: 0,
            top: 0,
            right: 1280,
            bottom: 800,
        };
        // Before winit creates the window, query the current desktop's logical work area.
        unsafe {
            if SystemParametersInfoW(0x0030, 0, &mut area as *mut Rect as *mut _, 0) != 0 {
                let scale = (GetDpiForSystem() as f32 / 96.).max(1.);
                return fit_size((
                    (area.right - area.left) as f32 / scale,
                    (area.bottom - area.top) as f32 / scale,
                ));
            }
        }
    }
    (1180., 720.)
}

pub fn fit_native_window(window: &rune::WindowHandle) {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct Point {
            x: i32,
            y: i32,
        }
        #[repr(C)]
        struct Rect {
            left: i32,
            top: i32,
            right: i32,
            bottom: i32,
        }
        #[repr(C)]
        struct MonitorInfo {
            size: u32,
            monitor: Rect,
            work: Rect,
            flags: u32,
        }
        #[link(name = "user32")]
        unsafe extern "system" {
            fn MonitorFromPoint(point: Point, flags: u32) -> *mut core::ffi::c_void;
            fn GetMonitorInfoW(monitor: *mut core::ffi::c_void, info: *mut MonitorInfo) -> i32;
        }
        let Some(p) = window.placement() else { return };
        let mut info = MonitorInfo {
            size: core::mem::size_of::<MonitorInfo>() as u32,
            monitor: Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            work: Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            flags: 0,
        };
        // placement and monitor rectangles are physical pixels; winit supplies this monitor's scale.
        let valid = unsafe {
            let monitor = MonitorFromPoint(
                Point {
                    x: p.content_position.0,
                    y: p.content_position.1,
                },
                2,
            );
            GetMonitorInfoW(monitor, &mut info) != 0
        };
        if !valid {
            return;
        }
        let scale = p.scale.max(1.);
        let available = fit_size((
            (info.work.right - info.work.left) as f32 / scale as f32,
            (info.work.bottom - info.work.top) as f32 / scale as f32,
        ));
        let current = (
            p.content_size.0 as f32 / scale as f32,
            p.content_size.1 as f32 / scale as f32,
        );
        if current.0 > available.0 || current.1 > available.1 {
            window.resize_instant((current.0.min(available.0), current.1.min(available.1)));
            window.center();
        }
    }
}
