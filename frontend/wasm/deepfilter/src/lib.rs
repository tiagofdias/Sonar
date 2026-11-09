use deep_filter_rt::tract::DfTract;
use ndarray::Array2;
pub struct Engine { df: DfTract, input: Array2<f32>, output: Array2<f32>, frame: [f32; 480] }
#[no_mangle] pub extern "C" fn df_create() -> *mut Engine {
    Box::into_raw(Box::new(Engine { df: DfTract::default(), input: Array2::zeros((1, 480)), output: Array2::zeros((1, 480)), frame: [0.0; 480] }))
}
#[no_mangle] pub unsafe extern "C" fn df_frame(e: *mut Engine) -> *mut f32 { (*e).frame.as_mut_ptr() }
#[no_mangle] pub unsafe extern "C" fn df_process(e: *mut Engine) {
    let e = &mut *e;
    e.input.row_mut(0).iter_mut().zip(e.frame.iter()).for_each(|(a, b)| *a = *b);
    if e.df.process(e.input.view(), e.output.view_mut()).is_ok() { e.frame.iter_mut().zip(e.output.row(0).iter()).for_each(|(a, b)| *a = *b); }
}
#[no_mangle] pub unsafe extern "C" fn df_destroy(e: *mut Engine) { drop(Box::from_raw(e)); }

// The model never needs randomness for security; this only satisfies a dependency.
#[no_mangle]
unsafe extern "Rust" fn __getrandom_v03_custom(dest: *mut u8, len: usize) -> Result<(), getrandom::Error> {
    core::ptr::write_bytes(dest, 7, len);
    Ok(())
}
