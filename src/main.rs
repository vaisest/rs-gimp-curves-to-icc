use lcms2::{Locale, Profile, Tag, ToneCurve, MLU};
use regex::Regex;
use std::{fs, path::PathBuf};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "GIMP Curve to ICC")]
struct Opt {
    #[structopt(parse(from_os_str))]
    curves_input: PathBuf,

    #[structopt(parse(from_os_str))]
    icc_output: Option<PathBuf>,
}

fn parse_u16_curve_vec(input: &str) -> Vec<u16> {
    return input
        .split(" ")
        .map(|it| it.parse::<f32>().expect("failed to parse number"))
        .map(|f| (f * (u16::MAX) as f32).round() as u16)
        .collect();
}

fn scale_u16_to_u8_range(input: u16) -> u8 {
    ((input as f32 / u16::MAX as f32) * u8::MAX as f32) as u8
}

fn read_curves(path: PathBuf) -> Vec<Vec<u16>> {
    println!("reading curve samples from {path:?}...");
    let text = fs::read_to_string(path).expect("could not open gimp_curve.txt");

    let re = Regex::new(r"\(samples \d+ (.*)\)\)\n").unwrap();
    // the data portion of (samples n data1 data2 data3...) in the file
    let caps: Vec<&str> = re
        .captures_iter(&text)
        .map(|it| it.get(1).unwrap().as_str())
        .collect();

    assert!(caps.len() >= 4, "could not parse 4 curves from file");

    let gray = parse_u16_curve_vec(caps[0]);
    assert!(gray.len() == 256);

    let rgb_values = caps[1..4].iter().map(|&list| parse_u16_curve_vec(list));

    rgb_values
        // apply gray curve to (after) rgb curves and and scale to u16
        .map(|vec| {
            vec.iter()
                .map(|&n| gray[scale_u16_to_u8_range(n) as usize])
                .collect::<Vec<u16>>()
        })
        .collect::<Vec<Vec<u16>>>()
}

fn main() {
    let opt = Opt::from_args();
    let mut icc = Profile::new_srgb();

    icc.remove_tag(lcms2::TagSignature::ProfileDescriptionTag);

    // description that is shown in Windows colour management
    let mut desc = MLU::new(1);
    desc.set_text(
        "sRGB profile with VCGT from GIMP curve tool",
        Locale::none(),
    );
    icc.write_tag(lcms2::TagSignature::ProfileDescriptionTag, Tag::MLU(&desc));

    // curves are exported from GIMP curve tool
    let rgb_curves = read_curves(opt.curves_input);

    let r_tc = ToneCurve::new_tabulated(&rgb_curves[0]);
    let g_tc = ToneCurve::new_tabulated(&rgb_curves[1]);
    let b_tc = ToneCurve::new_tabulated(&rgb_curves[2]);

    // println!("{:?}", b_tc.estimated_entries());

    let tc_refs: [&lcms2::ToneCurveRef; 3] = [&r_tc, &g_tc, &b_tc];
    let vcgt_tag = Tag::VcgtCurves(tc_refs);
    icc.write_tag(lcms2::TagSignature::VcgtTag, vcgt_tag);

    let dest = opt.icc_output.unwrap_or(PathBuf::from("out.icc"));
    println!("saving profile to out.icc...");
    icc.save_profile_to_file(dest.as_path())
        .expect("error while saving profile");
}
