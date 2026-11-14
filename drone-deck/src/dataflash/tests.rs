use super::fixture::{Builder, F};
use super::*;

const PARM: u8 = 64;
const GUIP: u8 = 65;
const ORGN: u8 = 66;
const IMU: u8 = 67;

fn log() -> Builder {
    let mut b = Builder::new();
    b.format(PARM, "PARM", "QNff", "TimeUS,Name,Value,Default")
        .format(
            GUIP,
            "GUIP",
            "QBfffbffffff",
            "TimeUS,Type,pX,pY,pZ,Terrain,vX,vY,vZ,aX,aY,aZ",
        )
        .format(ORGN, "ORGN", "QBLLe", "TimeUS,Type,Lat,Lng,Alt")
        .format(IMU, "IMU", "Qfff", "TimeUS,GyrX,GyrY,GyrZ");
    b
}

#[test]
fn rows_are_read_by_column_with_pymavlink_scaling() {
    let mut b = log();
    b.row(
        PARM,
        &[F::N(1000.), F::S("WPNAV_SPEED"), F::N(1000.), F::N(1000.)],
    )
    .row(
        ORGN,
        &[
            F::N(2000.),
            F::N(0.),
            F::N(-35.3632621),
            F::N(149.1652374),
            F::N(584.07),
        ],
    )
    .row(IMU, &[F::N(3000.), F::N(0.1), F::N(0.2), F::N(0.3)]);
    let log = parse(&b.bytes(), &["PARM", "ORGN"]).unwrap();
    let parm: Vec<_> = log.rows("PARM").collect();
    assert_eq!(parm.len(), 1);
    assert_eq!(parm[0].text("Name"), Some("WPNAV_SPEED"));
    assert_eq!(parm[0].f64("Value"), Some(1000.));
    assert_eq!(parm[0].time_us(), Some(1000));
    let orgn = log.rows("ORGN").next().unwrap();
    assert!(
        (orgn.f64("Lat").unwrap() + 35.3632621).abs() < 1e-7,
        "L is 1e-7 degrees"
    );
    assert!(
        (orgn.f64("Alt").unwrap() - 584.07).abs() < 1e-9,
        "e is hundredths"
    );
    assert_eq!(log.count("IMU"), 0, "only the wanted messages are kept");
    assert!(log.formats.contains_key("IMU"), "every format is known");
    assert_eq!(log.skipped, 0);
}

#[test]
fn fmtu_multipliers_scale_raw_columns_to_their_units() {
    // Copter 4.6 logs guided targets in centimetres (multiplier B) and its TimeUS in µs (F).
    let mut b = log();
    b.units(GUIP, "F-BBB-BBBBBB")
        .mult(b'B', 0.01)
        .mult(b'F', 1e-6);
    b.row(
        GUIP,
        &[
            F::N(5_000_000.),
            F::N(4.),
            F::N(0.),
            F::N(0.),
            F::N(0.),
            F::N(0.),
            F::N(150.),
            F::N(-50.),
            F::N(20.),
            F::N(0.),
            F::N(0.),
            F::N(0.),
        ],
    );
    let log = parse(&b.bytes(), &["GUIP"]).unwrap();
    let row = log.rows("GUIP").next().unwrap();
    assert_eq!(row.time_us(), Some(5_000_000), "TimeUS stays in µs");
    assert_eq!(row.i64("Type"), Some(4), "a column without a multiplier");
    assert!((row.f64("vX").unwrap() - 1.5).abs() < 1e-9);
    assert!((row.f64("vY").unwrap() + 0.5).abs() < 1e-9);
    assert!((row.f64("vZ").unwrap() - 0.2).abs() < 1e-9);
}

#[test]
fn damage_is_skipped_and_a_cut_off_message_dropped() {
    let mut b = log();
    b.raw(&[0xA3, 0x00, 0x13, 0x37])
        .row(IMU, &[F::N(1.), F::N(0.), F::N(0.), F::N(0.)])
        .raw(&[0xA3, 0x95, 0xEE])
        .row(IMU, &[F::N(2.), F::N(0.), F::N(0.), F::N(0.)]);
    let mut bytes = b.bytes();
    bytes.extend([0xA3, 0x95, IMU, 0, 0]);
    let log = parse(&bytes, &["IMU"]).unwrap();
    let times: Vec<_> = log.rows("IMU").map(|r| r.time_us().unwrap()).collect();
    assert_eq!(times, [1, 2]);
    assert!(log.skipped >= 4);
}

#[test]
fn other_files_are_refused() {
    assert_eq!(
        parse(b"ULog\x01\x12\x35 not a dataflash log", &[]).unwrap_err(),
        Error::NotDataFlash
    );
    assert!(Error::NotDataFlash.to_string().contains("DataFlash"));
}
