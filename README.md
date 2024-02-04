# Gimp curves to ICC with gamma table conversion tool

This repository contains a short Rust program that will convert input text files as in `tarky_curve.txt` to sRGB ICC profiles with an embedded gamma table. This is mainly useful as a way of adding custom calibration to the Windows desktop, which is not colour managed, but does support loading a gamma table onto the GPU. This is similar to how Nvidia control panel can change gamma, but the result should be much more customisable and less ugly.

Running this app will require Rust. For install instructions see [here](https://www.rust-lang.org/tools/install). If you want to make your own curves, you will also need [Gimp](https://www.gimp.org/downloads/) and some sample screenshots. The instructions for the curve tool can be found [here](https://docs.gimp.org/en/gimp-tool-curves.html). The curve should be saved in the new format, and note that the alpha channel is ignored, but Value + RGB is supported.

Example command to run the app: `./rs-gimp-to-icc.exe -d "A test ICC profile" tarky_curve.txt tarky.icc` or `cargo run --release -- tarky_curve.txt tarky.icc`. You can download a prebuilt binary from the [releases page](https://github.com/vaisest/rs-gimp-curves-to-icc/releases).

For instructions on how to apply an ICC profile see [this Microsoft support article](https://support.microsoft.com/en-us/windows/about-color-management-2a2ed8fa-cf09-83c5-e55c-d1428519f616).
