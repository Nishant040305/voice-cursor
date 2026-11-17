use std::fs;

pub fn process_cpu_seconds() -> f64 {
    // RUSAGE_SELF includes CPU time spent by the ONNX Runtime worker threads.
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if result != 0 {
        return 0.0;
    }
    let usage = unsafe { usage.assume_init() };
    timeval_seconds(usage.ru_utime) + timeval_seconds(usage.ru_stime)
}

fn timeval_seconds(value: libc::timeval) -> f64 {
    value.tv_sec as f64 + value.tv_usec as f64 / 1_000_000.0
}

pub fn rss_now() -> String {
    let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find(|line| line.starts_with("VmRSS:"))
        .map(str::to_owned)
        .unwrap_or_else(|| "unavailable".into())
}

pub fn peak_rss() -> String {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return "unavailable".into();
    }
    let peak_kib = unsafe { usage.assume_init() }.ru_maxrss;
    format!("{:.1} MiB", peak_kib as f64 / 1024.0)
}
