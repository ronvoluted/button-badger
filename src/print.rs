//! Direct printing through the platform's own print dialog.
//! macOS: PDFKit print operation (vector PDF, scale 100%). Windows: PrintDlg + GDI raster, sent in bands.
//! Elsewhere: the XDG print portal, falling back to the default PDF viewer.

#[cfg(any(target_os = "macos", windows))]
use crate::pdf::Document;

// Each `print` shows the system print dialog and prints, returning a status line for the toast.
// On macOS and Windows it blocks while the dialog is open and must be called from the main thread.
// The portal version is async.

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
    use crate::raster::{Badges, page_px, render_band};
    use std::mem::size_of;
    use windows::Win32::Foundation::GlobalFree;
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteDC, GetDeviceCaps, LOGPIXELSX, LOGPIXELSY, PHYSICALOFFSETX,
        PHYSICALOFFSETY, SRCCOPY, StretchDIBits, VERTRES,
    };
    use windows::Win32::Storage::Xps::{AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage};
    use windows::Win32::UI::Controls::Dialogs::{PD_NOPAGENUMS, PD_NOSELECTION, PD_RETURNDC, PD_USEDEVMODECOPIESANDCOLLATE, PRINTDLGW, PrintDlgW};
    use windows::core::PCWSTR;

    unsafe {
        let mut pd = PRINTDLGW {
            lStructSize: size_of::<PRINTDLGW>() as u32,
            Flags: PD_RETURNDC | PD_USEDEVMODECOPIESANDCOLLATE | PD_NOPAGENUMS | PD_NOSELECTION,
            nCopies: 1,
            ..Default::default()
        };
        let chosen = PrintDlgW(&mut pd).as_bool();
        // Only the device context is used. The dialog also allocates two settings blocks (even when cancelled)
        // that are ours to free.
        let _ = GlobalFree(Some(pd.hDevMode));
        let _ = GlobalFree(Some(pd.hDevNames));
        if !chosen {
            return Ok("Print cancelled".into());
        }
        let hdc = pd.hDC;
        let dpi = (GetDeviceCaps(Some(hdc), LOGPIXELSX) as f32, GetDeviceCaps(Some(hdc), LOGPIXELSY) as f32);
        // Device coordinates start at the printable area, `off` pixels in from the paper's corner.
        let (off_x, off_y) = (GetDeviceCaps(Some(hdc), PHYSICALOFFSETX), GetDeviceCaps(Some(hdc), PHYSICALOFFSETY));
        let printable_rows = GetDeviceCaps(Some(hdc), VERTRES);
        let (w, h) = page_px(doc, dpi);
        // A whole page at printer resolution is ~140 MB, so it is rendered and sent in bands of about 8 MB.
        let band_rows = ((8 << 20) / (w * 4)).max(1);

        let title: Vec<u16> = doc.title.encode_utf16().chain([0]).collect();
        let di = DOCINFOW { cbSize: size_of::<DOCINFOW>() as i32, lpszDocName: PCWSTR(title.as_ptr()), ..Default::default() };
        if StartDocW(hdc, &di) <= 0 {
            let _ = DeleteDC(hdc);
            return Err("The printer refused the job".into());
        }
        // Every GDI call here reports failure as zero or less.
        let send = || {
            let mut badges = Badges::new();
            for n in 0..doc.page_count() {
                if StartPage(hdc) <= 0 {
                    return false;
                }
                for y0 in (0..h).step_by(band_rows as usize) {
                    let rows = band_rows.min(h - y0);
                    let top = y0 as i32 - off_y;
                    if top + rows as i32 <= 0 || top >= printable_rows {
                        continue; // wholly in the unprintable margin: nothing to send (and it would copy 0 lines)
                    }
                    let mut band = render_band(doc, n, dpi, y0, rows, &mut badges);
                    // RGBA → BGRA in place. Alpha is already 255 (the band is opaque).
                    for p in band.pixels_mut() {
                        p.0.swap(0, 2);
                    }
                    let bmi = BITMAPINFO {
                        bmiHeader: BITMAPINFOHEADER {
                            biSize: size_of::<BITMAPINFOHEADER>() as u32,
                            biWidth: w as i32,
                            biHeight: -(rows as i32), // negative: rows run top-down
                            biPlanes: 1,
                            biBitCount: 32,
                            biCompression: BI_RGB.0,
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    let bits = Some(band.as_raw().as_ptr().cast());
                    if StretchDIBits(hdc, -off_x, top, w as i32, rows as i32, 0, 0, w as i32, rows as i32, bits, &bmi, DIB_RGB_COLORS, SRCCOPY) <= 0 {
                        return false;
                    }
                }
                if EndPage(hdc) <= 0 {
                    return false;
                }
            }
            EndDoc(hdc) > 0
        };
        let sent = send();
        if !sent {
            AbortDoc(hdc);
        }
        let _ = DeleteDC(hdc);
        if sent { Ok("Sent to the printer".into()) } else { Err("The printer stopped partway through the job".into()) }
    }
}

/// Linux and other Unix desktops: the desktop's own print dialog through the XDG print portal. Where there is
/// no portal, the PDF is handed to the default viewer instead.
#[cfg(not(any(target_os = "macos", windows)))]
pub async fn print(pdf: Vec<u8>, title: String) -> Result<String, String> {
    use ashpd::desktop::ResponseError;
    // One fixed file, overwritten by every job, so nothing piles up in the temp directory.
    let path = std::env::temp_dir().join("button-badger-print.pdf");
    std::fs::write(&path, pdf).map_err(|e| e.to_string())?;
    match portal_print(&path, &title).await {
        // The portal has the file open by now, so the name can go.
        Ok(()) => {
            let _ = std::fs::remove_file(&path);
            Ok("Sent to the printer".into())
        }
        Err(ashpd::Error::Response(ResponseError::Cancelled)) => {
            let _ = std::fs::remove_file(&path);
            Ok("Print cancelled".into())
        }
        // No usable portal. The viewer reads the file after we return, so it stays (and is reused next time).
        Err(_) => {
            std::process::Command::new("xdg-open").arg(&path).spawn().map_err(|e| e.to_string())?;
            Ok("Opened in your PDF viewer — print from there at 100% scale".into())
        }
    }
}

/// Show the portal's print dialog, then send it the PDF at `path`.
#[cfg(not(any(target_os = "macos", windows)))]
async fn portal_print(path: &std::path::Path, title: &str) -> Result<(), ashpd::Error> {
    use ashpd::desktop::print::{PageSetup, PreparePrintOptions, PrintOptions, PrintProxy, Settings};
    let proxy = PrintProxy::new().await?;
    // Paper and scaling are left at the desktop's defaults for the user to set, as in any other app: the
    // portal's paper hints differ between desktops, and a wrong hint is worse than none.
    let prepare = PreparePrintOptions::default().set_modal(true);
    let chosen = proxy.prepare_print(None, title, Settings::default(), PageSetup::default(), prepare).await?.response()?;
    let file = std::fs::File::open(path).map_err(ashpd::Error::IO)?;
    let options = PrintOptions::default().set_token(chosen.token).set_modal(true);
    proxy.print(None, title, &file, options).await?.response()?;
    Ok(())
}

/// Show a file in the OS file manager.
pub fn reveal(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg("-R").arg(path);
        c
    };
    #[cfg(windows)]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        // Explorer wants `/select,"C:\a b\c.pdf"`. `arg` would quote the whole thing as
        // `"/select,C:\a b\c.pdf"` when the path has a space, which Explorer doesn't parse,
        // so it opens Documents instead. Windows paths can't contain `"`, so this quoting is safe.
        let mut c = std::process::Command::new("explorer");
        c.raw_arg(format!("/select,\"{}\"", path.display()));
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(path.parent().unwrap_or(path));
        c
    };
    let _ = cmd.spawn();
}
