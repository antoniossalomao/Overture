//! Render headless da tela principal para PPM: `cargo run -p overture-ui --example snap -- W H out.ppm`.
use std::{io::Write, rc::Rc};

use overture_ui::MainWindow;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType, Rgb565Pixel};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, PhysicalSize, PlatformError};

struct P(Rc<MinimalSoftwareWindow>);
impl Platform for P {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.0.clone())
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (w, h): (u32, u32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let win = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(P(win.clone()))).unwrap();
    let ui = MainWindow::new().unwrap();
    win.set_size(PhysicalSize::new(w, h));
    ui.show().unwrap();
    let mut buf = vec![slint::Rgb8Pixel::default(); (w * h) as usize];
    win.draw_if_needed(|r| {
        r.render(&mut buf, w as usize);
    });
    let _ = Rgb565Pixel::default();
    let mut f = std::fs::File::create(&a[3]).unwrap();
    write!(f, "P6\n{w} {h}\n255\n").unwrap();
    for p in &buf {
        f.write_all(&[p.r, p.g, p.b]).unwrap();
    }
}
