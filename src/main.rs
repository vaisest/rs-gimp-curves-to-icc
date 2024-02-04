use clap::Parser;
use lcms2::{Locale, Profile, Tag, ToneCurve, MLU};
use regex::Regex;
use std::{fs, path::PathBuf};

#[derive(Parser, Debug)]
#[command(name = "GIMP Curve to ICC")]
struct Args {
    /// Input file name
    #[arg()]
    curves_input: PathBuf,

    /// Toggle for sampling from 3D LUT input instead of GIMP curves.
    /// Input must be in .cube format.
    #[arg(long = "3dlut", default_value_t = false)]
    input_is_3dlut: bool,

    /// Output file name
    #[arg(default_value = "out.icc")]
    icc_output: PathBuf,

    /// Description or name that will appear in Windows' colour management menu
    #[arg(
        short,
        long = "description",
        default_value = "sRGB profile with VCGT from GIMP curve tool"
    )]
    description: String,
}

/// Parses e.g. "0.0 0.001 0.033 ..." to vec of numbers scaled from 0 to 65535
fn parse_u16_curve_vec(input: &str) -> Vec<u16> {
    return input
        .split(" ")
        .map(|it| it.parse::<f32>().expect("failed to parse number"))
        .map(|f| (f * (u16::MAX) as f32).round() as u16)
        .collect();
}

/// Scales from 0-65535 to 0-255
fn scale_u16_to_u8_range(input: u16) -> u8 {
    ((input as f32 / u16::MAX as f32) * u8::MAX as f32) as u8
}

/// Parses GIMP's new curve format which is formatted in a LISP-like way
fn parse_curves(text: String) -> Vec<Vec<u16>> {
    // gimp seems to be able to save linear curves which will probably look wrong
    if text.contains("linear yes") {
        println!("Curve input is saved in linear light. The result might not look correct")
    }

    // mR flags: multi-line and CRLF mode
    let re = Regex::new(r"(?Rm)^ *\(samples \d+ (.*)\)\)$").unwrap();
    // gets us the values portion of (samples n value1 value2 value3...) in the file
    let caps: Vec<&str> = re
        .captures_iter(&text)
        .map(|it| it.get(1).unwrap().as_str())
        .collect();

    // 1 value curve (gray), and 3 colour curves (R, G, B). Possibly also alpha but that is ignored
    assert!(
        caps.len() >= 4,
        "Could not parse 4 curves from file. Exiting..."
    );

    let gray = parse_u16_curve_vec(caps[0]);
    // GIMP doesn't seem to save curves of different accuracy
    assert!(gray.len() == 256);

    let rgb_values = caps[1..4].iter().map(|&list| parse_u16_curve_vec(list));

    rgb_values
        // apply gray curve to the RGB curves, reducing 4 curves to 3 colour channel curves
        .map(|color_curve| {
            color_curve
                .iter()
                // values are scaled down to 0-255 as there are 256 values in each curve and used as gray input values
                .map(|&color_value| gray[scale_u16_to_u8_range(color_value) as usize])
                .collect::<Vec<u16>>()
        })
        .collect::<Vec<Vec<u16>>>()
}

fn parse_3dlut(text: String) -> Vec<Vec<u16>> {
    // this is not well parsed.
    // full spec here: https://resolve.cafe/developers/luts/

    // get LUT size
    let size_re = Regex::new(r"LUT_3D_SIZE (\d+)").unwrap();

    let lut_size = size_re
        .captures(&text)
        .expect("Input 3DLUT could not be parsed. Make sure it is in Resolve .cube format.")[1]
        .parse::<usize>()
        .unwrap();

    // parses the lines of float triplets
    // like "0.684306 0.927321 0.955734" or mixed with integers "0.1 0 1" or with exp notation "0.901122 7.62951e-05 0.583398"
    let triplet_re = Regex::new(
        r"(?i)(\d.\d+|\d|\d.\d+e-?\d+) (\d.\d+|\d|\d.\d+e-?\d+) (\d.\d+|\d|\d.\d+e-?\d+)",
    )
    .unwrap();

    // idx for start of the data portion of the file
    let idx_start = triplet_re.find(&text).unwrap().start();
    let values = text[idx_start..]
        .split_whitespace()
        .filter_map(|it| it.parse::<f64>().ok());

    let mut rgb_values: Vec<[f64; 3]> = Vec::with_capacity(lut_size.pow(3));

    // chunking by 3 so we at least have a vec of triplets
    let mut counter = 0;
    let mut buffer = [0.0f64; 3];
    for val in values {
        buffer[counter] = val;

        if counter == 2 {
            counter = 0;
            rgb_values.push(buffer);
        } else {
            counter += 1;
        }
    }

    assert!(counter == 0);

    assert_eq!(lut_size.pow(3), rgb_values.len());

    let mut curves = Vec::new();

    // channel we're sampling now. from red to blue
    for channel in 0..=2 {
        let mut curve = Vec::new();
        for a in 0..=255 {
            // scale a to 0..lut_size

            // x is a point between n-1 and n. We will have to linearly interpolate
            // this point x as the LUT is just lut_size values instead of 255 values
            let x = (a as f64 / 255 as f64) * (lut_size - 1) as f64;
            // n-1, or leftmost closest point
            let ix = x as usize;
            // distance of x from n
            let dx = x - ix as f64;

            let (left, right) = match channel {
                // red
                0 => (
                    rgb_values[ix][channel],
                    rgb_values[(ix + 1).clamp(0, lut_size - 1)][channel],
                ),
                // green
                1 => (
                    rgb_values[ix * lut_size][channel],
                    rgb_values[(ix + 1).clamp(0, lut_size - 1) * lut_size][channel],
                ),
                // blue
                2 => (
                    rgb_values[ix * lut_size.pow(2)][channel],
                    rgb_values[(ix + 1).clamp(0, lut_size - 1) * lut_size.pow(2)][channel],
                ),
                _ => unreachable!(),
            };
            // weigh closest two points based on distance of x from n
            let val = left * dx + right * (1.0 - dx);
            // sale to u16 range and output
            println!(
                "in: {a}, out: {}, x: {x}",
                (val * u16::MAX as f64).round() as u16
            );
            curve.push((val * u16::MAX as f64).round() as u16);
        }
        curves.push(curve);
    }
    curves
}

fn main() {
    let args = Args::parse();
    let mut icc = Profile::new_srgb();

    icc.remove_tag(lcms2::TagSignature::ProfileDescriptionTag);

    // description that is shown in Windows colour management
    let mut desc = MLU::new(1);
    desc.set_text(&args.description, Locale::none());
    icc.write_tag(lcms2::TagSignature::ProfileDescriptionTag, Tag::MLU(&desc));

    // curves are exported from GIMP curve tool
    println!("reading curve samples from {:?}...", &args.curves_input);
    let text = fs::read_to_string(&args.curves_input)
        .unwrap_or_else(|err| panic!("Could not read file {:?}: {}", args.curves_input, err));

    let rgb_curves = if args.input_is_3dlut {
        parse_3dlut(text)
    } else {
        parse_curves(text)
    };

    println!("{rgb_curves:?}");

    let r_tc = ToneCurve::new_tabulated(&rgb_curves[0]);
    let g_tc = ToneCurve::new_tabulated(&rgb_curves[1]);
    let b_tc = ToneCurve::new_tabulated(&rgb_curves[2]);

    let tc_refs: [&lcms2::ToneCurveRef; 3] = [&r_tc, &g_tc, &b_tc];
    let vcgt_tag = Tag::VcgtCurves(tc_refs);
    icc.write_tag(lcms2::TagSignature::VcgtTag, vcgt_tag);

    println!("saving profile to {:?}...", args.icc_output);
    icc.save_profile_to_file(args.icc_output.as_path())
        .unwrap_or_else(|err| panic!("Error while saving profile to: {err}",));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// Tests the entirety of parse_curves() with a known example
    fn parsing_example_input_works() {
        let input = fs::read_to_string("test/gimp_test_curve.txt").unwrap();
        let parsed_result = parse_curves(input);

        // big array
        let expected = vec![
            vec![
                0, 0, 0, 0, 232, 463, 696, 928, 1161, 1395, 1630, 1867, 2104, 2343, 2584, 2826,
                3070, 3317, 3565, 3816, 4070, 4327, 4586, 4848, 5120, 5387, 5655, 5923, 6192, 6461,
                6730, 7000, 7270, 7542, 7813, 8086, 8359, 8634, 8909, 9185, 9462, 9741, 10020,
                10301, 10583, 10866, 11151, 11437, 11725, 12014, 12305, 12597, 12891, 13187, 13485,
                13785, 14087, 14390, 14696, 14848, 15156, 15466, 15776, 16088, 16401, 16714, 17029,
                17345, 17661, 17978, 18296, 18615, 18935, 19255, 19576, 19898, 20221, 20544, 20868,
                21192, 21517, 21843, 22168, 22495, 22822, 23149, 23477, 23805, 24133, 24462, 24791,
                25120, 25449, 25779, 26109, 26439, 26769, 27099, 27429, 27759, 28089, 28420, 28750,
                29080, 29410, 29739, 30069, 30398, 30728, 31056, 31385, 31713, 32041, 32369, 32696,
                33023, 33350, 33675, 34001, 34326, 34650, 34974, 35297, 35619, 35941, 36262, 36583,
                36902, 37221, 37539, 37856, 38172, 38488, 38802, 39116, 39428, 39740, 40051, 40360,
                40668, 40976, 41282, 41587, 41890, 42193, 42494, 42794, 43093, 43390, 43686, 43981,
                44274, 44565, 44856, 45144, 45432, 45717, 46001, 46283, 46564, 46847, 47123, 47394,
                47660, 47921, 48178, 48430, 48678, 48922, 49162, 49398, 49630, 49858, 50083, 50305,
                50524, 50739, 50952, 51161, 51368, 51573, 51775, 51974, 52172, 52367, 52561, 52752,
                52943, 53131, 53318, 53504, 53689, 53873, 54056, 54238, 54420, 54601, 54782, 54963,
                55144, 55324, 55505, 55687, 55868, 56051, 56234, 56418, 56603, 56789, 56976, 57165,
                57355, 57547, 57855, 58050, 58245, 58438, 58631, 58823, 59014, 59205, 59394, 59583,
                59772, 59959, 60146, 60333, 60518, 60703, 60888, 61072, 61255, 61438, 61621, 61803,
                61984, 62166, 62346, 62527, 62707, 62886, 63066, 63245, 63424, 63602, 63780, 63959,
                64136, 64314, 64492, 64669, 64847, 65024, 65201, 65378, 65535,
            ],
            vec![
                0, 0, 0, 0, 232, 463, 696, 928, 1161, 1395, 1630, 1867, 2104, 2343, 2584, 2826,
                3070, 3317, 3565, 3816, 4070, 4327, 4586, 4848, 5120, 5387, 5655, 5923, 6192, 6461,
                6730, 7000, 7270, 7542, 7813, 8086, 8359, 8634, 8909, 9185, 9462, 9741, 10020,
                10301, 10583, 10866, 11151, 11437, 11725, 12014, 12305, 12597, 12891, 13187, 13485,
                13785, 14087, 14390, 14696, 14848, 15156, 15466, 15776, 16088, 16401, 16714, 17029,
                17345, 17661, 17978, 18296, 18615, 18935, 19255, 19576, 19898, 20221, 20544, 20868,
                21192, 21517, 21843, 22168, 22495, 22822, 23149, 23477, 23805, 24133, 24462, 24791,
                25120, 25449, 25779, 26109, 26439, 26769, 27099, 27429, 27759, 28089, 28420, 28750,
                29080, 29410, 29739, 30069, 30398, 30728, 31056, 31385, 31713, 32041, 32369, 32696,
                33023, 33350, 33675, 34001, 34326, 34650, 34974, 35297, 35619, 35941, 36262, 36583,
                36902, 37221, 37539, 37856, 38172, 38488, 38802, 39116, 39428, 39740, 40051, 40360,
                40668, 40976, 41282, 41587, 41890, 42193, 42494, 42794, 43093, 43390, 43686, 43981,
                44274, 44565, 44856, 45144, 45432, 45717, 46001, 46283, 46564, 46847, 47123, 47394,
                47660, 47921, 48178, 48430, 48678, 48922, 49162, 49398, 49630, 49858, 50083, 50305,
                50524, 50739, 50952, 51161, 51368, 51573, 51775, 51974, 52172, 52367, 52561, 52752,
                52943, 53131, 53318, 53504, 53689, 53873, 54056, 54238, 54420, 54601, 54782, 54963,
                55144, 55324, 55505, 55687, 55868, 56051, 56234, 56418, 56603, 56789, 56976, 57165,
                57355, 57547, 57855, 58050, 58245, 58438, 58631, 58823, 59014, 59205, 59394, 59583,
                59772, 59959, 60146, 60333, 60518, 60703, 60888, 61072, 61255, 61438, 61621, 61803,
                61984, 62166, 62346, 62527, 62707, 62886, 63066, 63245, 63424, 63602, 63780, 63959,
                64136, 64314, 64492, 64669, 64847, 65024, 65201, 65378, 65535,
            ],
            vec![
                0, 0, 0, 0, 232, 463, 696, 928, 1161, 1395, 1630, 1867, 2104, 2343, 2584, 2826,
                3070, 3317, 3565, 3816, 4070, 4327, 4586, 4848, 5120, 5387, 5655, 5923, 6192, 6461,
                6730, 7000, 7270, 7542, 7813, 8086, 8359, 8634, 8909, 9185, 9462, 9741, 10020,
                10301, 10583, 10866, 11151, 11437, 11725, 12014, 12305, 12597, 12891, 13187, 13485,
                13785, 14087, 14390, 14696, 14848, 15156, 15466, 15776, 16088, 16401, 16714, 17029,
                17345, 17661, 17978, 18296, 18615, 18935, 19255, 19576, 19898, 20221, 20544, 20868,
                21192, 21517, 21843, 22168, 22495, 22822, 23149, 23477, 23805, 24133, 24462, 24791,
                25120, 25449, 25779, 26109, 26439, 26769, 27099, 27429, 27759, 28089, 28420, 28750,
                29080, 29410, 29739, 30069, 30398, 30728, 31056, 31385, 31713, 32041, 32369, 32696,
                33023, 33350, 33675, 34001, 34326, 34650, 34974, 35297, 35619, 35941, 36262, 36583,
                36902, 37221, 37539, 37856, 38172, 38488, 38802, 39116, 39428, 39740, 40051, 40360,
                40668, 40976, 41282, 41587, 41890, 42193, 42494, 42794, 43093, 43390, 43686, 43981,
                44274, 44565, 44856, 45144, 45432, 45717, 46001, 46283, 46564, 46847, 47123, 47394,
                47660, 47921, 48178, 48430, 48678, 48922, 49162, 49398, 49630, 49858, 50083, 50305,
                50524, 50739, 50952, 51161, 51368, 51573, 51775, 51974, 52172, 52367, 52561, 52752,
                52943, 53131, 53318, 53504, 53689, 53873, 54056, 54238, 54420, 54601, 54782, 54963,
                55144, 55324, 55505, 55687, 55868, 56051, 56234, 56418, 56603, 56789, 56976, 57165,
                57355, 57547, 57855, 58050, 58245, 58438, 58631, 58823, 59014, 59205, 59394, 59583,
                59772, 59959, 60146, 60333, 60518, 60703, 60888, 61072, 61255, 61438, 61621, 61803,
                61984, 62166, 62346, 62527, 62707, 62886, 63066, 63245, 63424, 63602, 63780, 63959,
                64136, 64314, 64492, 64669, 64847, 65024, 65201, 65378, 65535,
            ],
        ];

        assert_eq!(parsed_result, expected);
    }
}
