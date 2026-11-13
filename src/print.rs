//! Direct printing through the platform's own print dialog.
//! macOS: PDFKit print operation (vector PDF, scale 100%). Windows: PrintDlg + GDI raster.
//! Elsewhere: hand a temporary PDF to the default viewer.

use crate::pdf::Document;

// Each `print` shows the system print dialog and prints, returning a status line for the toast.
// It blocks while the dialog is open; call it from the main thread.

#[cfg(target_os = "macos")]
pub fn print(doc: &Document) -> Result<String, String> {
    use crate::layout::PT_PER_MM;
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSPaperOrientation, NSPrintInfo};
    use objc2_foundation::{NSData, NSSize, NSString};
    use objc2_pdf_kit::{PDFDocument, PDFPrintScalingMode};

    let mtm = MainThreadMarker::new().ok_or("Printing must start on the main thread")?;
    let bytes = doc.pdf();
    let data = NSData::with_bytes(&bytes);
    let pdf = unsafe { PDFDocument::initWithData(PDFDocument::alloc(), &data) }.ok_or("Could not build the PDF")?;

    let (w, h) = doc.sheet.page_mm();
    let info = NSPrintInfo::sharedPrintInfo();
    info.setPaperSize(NSSize::new((w * PT_PER_MM) as f64, (h * PT_PER_MM) as f64));
    info.setOrientation(if w > h { NSPaperOrientation::Landscape } else { NSPaperOrientation::Portrait });
    info.setLeftMargin(0.0);
    info.setRightMargin(0.0);
    info.setTopMargin(0.0);
    info.setBottomMargin(0.0);
    info.setScalingFactor(1.0);
    info.setHorizontallyCentered(true);
    info.setVerticallyCentered(true);

    let op = unsafe { pdf.printOperationForPrintInfo_scalingMode_autoRotate(Some(&info), PDFPrintScalingMode::PageScaleNone, false, mtm) }
        .ok_or("Could not create a print job")?;
    op.setShowsPrintPanel(true);
    op.setShowsProgressPanel(true);
    op.setJobTitle(Some(&NSString::from_str(doc.title)));
    Ok(if op.runOperation() { "Sent to the printer".into() } else { "Print cancelled".into() })
}

#[cfg(windows)]
pub fn print(doc: &Document) -> Result<String, String> {
    use std::mem::size_of;
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteDC, GetDeviceCaps, LOGPIXELSX, LOGPIXELSY, PHYSICALOFFSETX,
        PHYSICALOFFSETY, SRCCOPY, StretchDIBits,
    };
    use windows::Win32::Storage::Xps::{DOCINFOW, EndDoc, EndPage, StartDocW, StartPage};
    use windows::Win32::UI::Controls::Dialogs::{PD_NOPAGENUMS, PD_NOSELECTION, PD_RETURNDC, PD_USEDEVMODECOPIESANDCOLLATE, PRINTDLGW, PrintDlgW};
    use windows::core::PCWSTR;

    unsafe {
        let mut pd = PRINTDLGW {
            lStructSize: size_of::<PRINTDLGW>() as u32,
            Flags: PD_RETURNDC | PD_USEDEVMODECOPIESANDCOLLATE | PD_NOPAGENUMS | PD_NOSELECTION,
            nCopies: 1,
            ..Default::default()
        };
        if !PrintDlgW(&mut pd).as_bool() {
            return Ok("Print cancelled".into());
        }
        let hdc = pd.hDC;
        let dpi = (GetDeviceCaps(Some(hdc), LOGPIXELSX) as f32, GetDeviceCaps(Some(hdc), LOGPIXELSY) as f32);
        let (off_x, off_y) = (GetDeviceCaps(Some(hdc), PHYSICALOFFSETX), GetDeviceCaps(Some(hdc), PHYSICALOFFSETY));
        let title: Vec<u16> = doc.title.encode_utf16().chain([0]).collect();
        let di = DOCINFOW { cbSize: size_of::<DOCINFOW>() as i32, lpszDocName: PCWSTR(title.as_ptr()), ..Default::default() };
        if StartDocW(hdc, &di) <= 0 {
            let _ = DeleteDC(hdc);
            return Err("The printer refused the job".into());
        }
        for n in 0..doc.page_count() {
            let mut img = crate::raster::render_page(doc, n, dpi);
            let (w, h) = img.dimensions();
            // RGBA → BGRA in place: a full page at printer DPI is ~140 MB, so don't hold a second copy.
            // Alpha is already 255 (render_page returns an opaque page).
            for p in img.pixels_mut() {
                p.0.swap(0, 2);
            }
            let bgra = img.as_raw();
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w as i32,
                    biHeight: -(h as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            StartPage(hdc);
            StretchDIBits(hdc, -off_x, -off_y, w as i32, h as i32, 0, 0, w as i32, h as i32, Some(bgra.as_ptr().cast()), &bmi, DIB_RGB_COLORS, SRCCOPY);
            EndPage(hdc);
        }
        EndDoc(hdc);
        let _ = DeleteDC(hdc);
    }
    Ok("Sent to the printer".into())
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn print(doc: &Document) -> Result<String, String> {
    let path = std::env::temp_dir().join(format!("button-badger-{}.pdf", std::process::id()));
    std::fs::write(&path, doc.pdf()).map_err(|e| e.to_string())?;
    std::process::Command::new("xdg-open").arg(&path).spawn().map_err(|e| e.to_string())?;
    Ok("Opened in your PDF viewer — print from there at 100% scale".into())
}

/// Show a file in the OS file manager.
pub fn reveal(path: &std::path::Path) {
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg("-R").arg(path);
        c
    } else if cfg!(windows) {
        let mut c = std::process::Command::new("explorer");
        c.arg(format!("/select,{}", path.display()));
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(path.parent().unwrap_or(path));
        c
    };
    let _ = cmd.spawn();
}
