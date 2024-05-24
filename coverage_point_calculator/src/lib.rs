///
/// Many changes to the rewards algorithm are contained in and across many HIPs.
/// The blog post [MOBILE Proof of Coverage][mobile-poc-blog] contains a more
/// thorough explanation of many of them. It is not exhaustive, but a great
/// place to start.
///
/// The coverage_points calculation in [`LocalRadio.coverage_points()`] are
/// comprised the following fields.
///
/// - estimated_coverage_points
///   - [HIP-74][modeled-coverage]
///   - reduced cbrs radio coverage points [HIP-113][cbrs-experimental]
/// - assignment_multiplier
///   - [HIP-103][oracle-boosting]
/// - rank
///   - [HIP-105][hex-limits]
/// - hex_boost_multiplier  
///   - [HIP-84][provider-boosting]
/// - location_trust_score_multiplier
///   - [HIP-98][qos-score]
/// - speedtest_multiplier
///   - [HIP-74][modeled-coverage]
///   - added "Good" speedtest tier [HIP-98][qos-score]
///
/// [modeled-coverage]:
///     https://github.com/helium/HIP/blob/main/0074-mobile-poc-modeled-coverage-rewards.md#outdoor-radios
/// [cbrs-experimental]:
///     https://github.com/helium/HIP/blob/main/0113-reward-cbrs-as-experimental.md
/// [oracle-boosting]:
///     https://github.com/helium/HIP/blob/main/0103-oracle-hex-boosting.md
/// [hex-limits]:
///     https://github.com/helium/HIP/blob/main/0105-modification-of-mobile-subdao-hex-limits.md
/// [provider-boosting]:
///     https://github.com/helium/HIP/blob/main/0084-service-provider-hex-boosting.md#mechanics-and-price-of-boosting-hexes
/// [qos-score]:
///     https://github.com/helium/HIP/blob/main/0098-mobile-subdao-quality-of-service-requirements.md
/// [mobile-poc-blog]:
///     https://docs.helium.com/mobile/proof-of-coverage
///
/// To Integrate in Docs:
///
/// Some verbiage about ranks.
/// https://github.com/helium/HIP/blob/main/0105-modification-of-mobile-subdao-hex-limits.md
///
/// Has something to say about 30meters from asserted location wrt poc rewards for boosted hexes.
/// https://github.com/helium/HIP/blob/8b1e814afa61a714b5ba63d3265e5897ab4c5116/0107-preventing-gaming-within-the-mobile-network.md
/// ! I cannot find a reason why the max distance to asserted changed from 30m to 50m.
///
pub mod speedtest;

use crate::speedtest::{Speedtest, SpeedtestTier};
use rust_decimal::{Decimal, RoundingStrategy};
use rust_decimal_macros::dec;

pub type Rank = std::num::NonZeroUsize;
type Multiplier = std::num::NonZeroU32;
pub type MaxOneMultplier = Decimal;
type Points = Decimal;

pub trait Radio {
    fn radio_type(&self) -> RadioType;
    fn speedtests(&self) -> Vec<Speedtest>;
    fn location_trust_scores(&self) -> Vec<LocationTrust>;
    fn verified_radio_threshold(&self) -> bool;
}

pub trait CoverageMap {
    fn hexes(&self, radio: &impl Radio) -> Vec<CoveredHex>;
}

pub fn calculate_coverage_points(radio: RewardableRadio) -> CoveragePoints {
    let radio_type = &radio.radio_type;

    let rank_multipliers = radio_type.rank_multipliers();
    let max_rank = rank_multipliers.len();

    let hex_points = radio
        .hexes
        .iter()
        .filter(|hex| hex.rank.get() <= max_rank)
        .map(|hex| {
            let base_coverage_points = radio_type.base_coverage_points(&hex.signal_level);
            let assignments_multiplier = hex.assignments.multiplier();
            let rank_multiplier = rank_multipliers[hex.rank.get() - 1];
            let hex_boost_multiplier = radio.hex_boosting_multiplier(hex);

            base_coverage_points * assignments_multiplier * rank_multiplier * hex_boost_multiplier
        });

    let base_points = hex_points.sum::<Decimal>();
    let location_score = radio.location_trust_multiplier();
    let speedtest = radio.speedtest_multiplier();

    let coverage_points = base_points * location_score * speedtest;
    let coverage_points = coverage_points.round_dp_with_strategy(2, RoundingStrategy::ToZero);

    CoveragePoints {
        coverage_points,
        radio,
    }
}

pub fn make_rewardable_radios<'a>(
    radios: &'a [impl Radio],
    coverage_map: &'a impl CoverageMap,
) -> impl Iterator<Item = RewardableRadio> + 'a {
    radios
        .iter()
        .map(|radio| make_rewardable_radio(radio, coverage_map))
}

pub fn make_rewardable_radio(
    radio: &impl Radio,
    coverage_map: &impl CoverageMap,
) -> RewardableRadio {
    RewardableRadio {
        radio_type: radio.radio_type(),
        speedtests: radio.speedtests(),
        location_trust_scores: radio.location_trust_scores(),
        verified_radio_threshold: radio.verified_radio_threshold(),
        hexes: coverage_map.hexes(radio),
    }
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct Meters(u32);

impl Meters {
    fn new(meters: u32) -> Self {
        Self(meters)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocationTrust {
    distance_to_asserted: Meters,
    trust_score: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RadioType {
    IndoorWifi,
    OutdoorWifi,
    IndoorCbrs,
    OutdoorCbrs,
}

impl RadioType {
    fn base_coverage_points(&self, signal_level: &SignalLevel) -> Points {
        match self {
            RadioType::IndoorWifi => match signal_level {
                SignalLevel::High => dec!(400),
                SignalLevel::Low => dec!(100),
                other => panic!("indoor wifi radios cannot have {other:?} signal levels"),
            },
            RadioType::OutdoorWifi => match signal_level {
                SignalLevel::High => dec!(16),
                SignalLevel::Medium => dec!(8),
                SignalLevel::Low => dec!(4),
                SignalLevel::None => dec!(0),
            },
            RadioType::IndoorCbrs => match signal_level {
                SignalLevel::High => dec!(100),
                SignalLevel::Low => dec!(25),
                other => panic!("indoor cbrs radios cannot have {other:?} signal levels"),
            },
            RadioType::OutdoorCbrs => match signal_level {
                SignalLevel::High => dec!(4),
                SignalLevel::Medium => dec!(2),
                SignalLevel::Low => dec!(1),
                SignalLevel::None => dec!(0),
            },
        }
    }

    fn rank_multipliers(&self) -> Vec<Decimal> {
        match self {
            RadioType::IndoorWifi => vec![dec!(1)],
            RadioType::IndoorCbrs => vec![dec!(1)],
            RadioType::OutdoorWifi => vec![dec!(1), dec!(0.5), dec!(0.25)],
            RadioType::OutdoorCbrs => vec![dec!(1), dec!(0.5), dec!(0.25)],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SignalLevel {
    High,
    Medium,
    Low,
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Assignments {
    pub footfall: Assignment,
    pub landtype: Assignment,
    pub urbanized: Assignment,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Assignment {
    A,
    B,
    C,
}

impl Assignments {
    fn multiplier(&self) -> MaxOneMultplier {
        let Assignments {
            footfall,
            urbanized,
            landtype,
        } = self;

        use Assignment::*;
        match (footfall, landtype, urbanized) {
            // yellow - POI ≥ 1 Urbanized
            (A, A, A) => dec!(1.00),
            (A, B, A) => dec!(1.00),
            (A, C, A) => dec!(1.00),
            // orange - POI ≥ 1 Not Urbanized
            (A, A, B) => dec!(1.00),
            (A, B, B) => dec!(1.00),
            (A, C, B) => dec!(1.00),
            // light green - Point of Interest Urbanized
            (B, A, A) => dec!(0.70),
            (B, B, A) => dec!(0.70),
            (B, C, A) => dec!(0.70),
            // dark green - Point of Interest Not Urbanized
            (B, A, B) => dec!(0.50),
            (B, B, B) => dec!(0.50),
            (B, C, B) => dec!(0.50),
            // light blue - No POI Urbanized
            (C, A, A) => dec!(0.40),
            (C, B, A) => dec!(0.30),
            (C, C, A) => dec!(0.05),
            // dark blue - No POI Not Urbanized
            (C, A, B) => dec!(0.20),
            (C, B, B) => dec!(0.15),
            (C, C, B) => dec!(0.03),
            // gray - Outside of USA
            (_, _, C) => dec!(0.00),
        }
    }
}

#[derive(Debug)]
pub struct CoveragePoints {
    pub coverage_points: Decimal,
    pub radio: RewardableRadio,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RewardableRadio {
    pub radio_type: RadioType,
    pub speedtests: Vec<Speedtest>,
    pub location_trust_scores: Vec<LocationTrust>,
    pub verified_radio_threshold: bool,
    pub hexes: Vec<CoveredHex>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoveredHex {
    pub rank: Rank,
    pub signal_level: SignalLevel,
    pub assignments: Assignments,
    pub boosted: Option<Multiplier>,
}

impl RewardableRadio {
    fn location_trust_multiplier(&self) -> Decimal {
        const RESTRICTIVE_MAX_DISTANCE: Meters = Meters(50);

        // CBRS radios are always trusted because they have internal GPS
        match self.radio_type {
            RadioType::IndoorCbrs => return dec!(1),
            RadioType::OutdoorCbrs => return dec!(1),
            _ => {}
        }

        let trust_score_sum: Decimal = if self.any_hexes_boosted() {
            // Cap multipliers to 0.25x when a radio covers _any_ boosted hex
            // and it's distance to asserted is above the threshold.
            self.location_trust_scores
                .iter()
                .map(|l| {
                    if l.distance_to_asserted > RESTRICTIVE_MAX_DISTANCE {
                        dec!(0.25).min(l.trust_score)
                    } else {
                        l.trust_score
                    }
                })
                .sum()
        } else {
            self.location_trust_scores
                .iter()
                .map(|l| l.trust_score)
                .sum()
        };

        let trust_score_count = Decimal::from(self.location_trust_scores.len());
        trust_score_sum / trust_score_count
    }

    fn hex_boosting_multiplier(&self, hex: &CoveredHex) -> MaxOneMultplier {
        let maybe_boost = if self.verified_radio_threshold {
            hex.boosted.map_or(1, |boost| boost.get())
        } else {
            1
        };
        Decimal::from(maybe_boost)
    }

    fn speedtest_multiplier(&self) -> MaxOneMultplier {
        const MIN_REQUIRED_SPEEDTEST_SAMPLES: usize = 2;

        if self.speedtests.len() < MIN_REQUIRED_SPEEDTEST_SAMPLES {
            return SpeedtestTier::Fail.multiplier();
        }

        let speedtest_avg = Speedtest::avg(&self.speedtests);
        speedtest_avg.multiplier()
    }

    fn any_hexes_boosted(&self) -> bool {
        self.hexes.iter().any(|hex| hex.boosted.is_some())
    }
}

#[cfg(test)]
mod tests {

    use crate::speedtest::{BytesPs, Millis};

    use super::*;
    use rust_decimal_macros::dec;

    impl Assignments {
        fn best() -> Self {
            Self {
                footfall: Assignment::A,
                landtype: Assignment::A,
                urbanized: Assignment::A,
            }
        }
    }

    impl Speedtest {
        fn best() -> Vec<Self> {
            vec![
                Self {
                    upload_speed: BytesPs::mbps(15),
                    download_speed: BytesPs::mbps(150),
                    latency: Millis::new(15),
                },
                Self {
                    upload_speed: BytesPs::mbps(15),
                    download_speed: BytesPs::mbps(150),
                    latency: Millis::new(15),
                },
            ]
        }
        fn download(download: BytesPs) -> Self {
            Self {
                upload_speed: BytesPs::mbps(15),
                download_speed: download,
                latency: Millis::new(15),
            }
        }
    }

    #[test]
    fn speedtest() {
        let mut indoor_cbrs = RewardableRadio {
            radio_type: RadioType::IndoorCbrs,
            speedtests: Speedtest::best(),
            location_trust_scores: vec![MaxOneMultplier::from_f32_retain(1.0).unwrap()],
            verified_radio_threshold: true,
            hexes: vec![CoveredHex {
                rank: Rank::new(1).unwrap(),
                signal_level: SignalLevel::High,
                assignments: Assignments::best(),
                boosted: None,
            }],
        };

        assert_eq!(
            dec!(100),
            calculate_coverage_points(indoor_cbrs.clone()).coverage_points
        );

        indoor_cbrs.speedtests = vec![
            Speedtest::download(BytesPs::mbps(88)),
            Speedtest::download(BytesPs::mbps(88)),
        ];
        assert_eq!(
            dec!(75),
            calculate_coverage_points(indoor_cbrs.clone()).coverage_points
        );

        indoor_cbrs.speedtests = vec![
            Speedtest::download(BytesPs::mbps(62)),
            Speedtest::download(BytesPs::mbps(62)),
        ];
        assert_eq!(
            dec!(50),
            calculate_coverage_points(indoor_cbrs.clone()).coverage_points
        );

        indoor_cbrs.speedtests = vec![
            Speedtest::download(BytesPs::mbps(42)),
            Speedtest::download(BytesPs::mbps(42)),
        ];
        assert_eq!(
            dec!(25),
            calculate_coverage_points(indoor_cbrs.clone()).coverage_points
        );

        indoor_cbrs.speedtests = vec![
            Speedtest::download(BytesPs::mbps(25)),
            Speedtest::download(BytesPs::mbps(25)),
        ];
        assert_eq!(
            dec!(0),
            calculate_coverage_points(indoor_cbrs).coverage_points
        );
    }

    #[test]
    fn oracle_boosting_assignments_apply_per_hex() {
        fn local_hex(
            footfall: Assignment,
            landtype: Assignment,
            urbanized: Assignment,
        ) -> CoveredHex {
            CoveredHex {
                rank: Rank::new(1).unwrap(),
                signal_level: SignalLevel::High,
                assignments: Assignments {
                    footfall,
                    landtype,
                    urbanized,
                },
                boosted: None,
            }
        }

        use Assignment::*;
        let indoor_cbrs = RewardableRadio {
            radio_type: RadioType::IndoorCbrs,
            speedtests: Speedtest::best(),
            location_trust_scores: vec![MaxOneMultplier::from_f32_retain(1.0).unwrap()],
            verified_radio_threshold: true,
            hexes: vec![
                // yellow - POI ≥ 1 Urbanized
                local_hex(A, A, A), // 100
                local_hex(A, B, A), // 100
                local_hex(A, C, A), // 100
                // orange - POI ≥ 1 Not Urbanized
                local_hex(A, A, B), // 100
                local_hex(A, B, B), // 100
                local_hex(A, C, B), // 100
                // light green - Point of Interest Urbanized
                local_hex(B, A, A), // 70
                local_hex(B, B, A), // 70
                local_hex(B, C, A), // 70
                // dark green - Point of Interest Not Urbanized
                local_hex(B, A, B), // 50
                local_hex(B, B, B), // 50
                local_hex(B, C, B), // 50
                // light blue - No POI Urbanized
                local_hex(C, A, A), // 40
                local_hex(C, B, A), // 30
                local_hex(C, C, A), // 5
                // dark blue - No POI Not Urbanized
                local_hex(C, A, B), // 20
                local_hex(C, B, B), // 15
                local_hex(C, C, B), // 3
                // gray - Outside of USA
                local_hex(A, A, C), // 0
                local_hex(A, B, C), // 0
                local_hex(A, C, C), // 0
                local_hex(B, A, C), // 0
                local_hex(B, B, C), // 0
                local_hex(B, C, C), // 0
                local_hex(C, A, C), // 0
                local_hex(C, B, C), // 0
                local_hex(C, C, C), // 0
            ],
        };

        assert_eq!(
            dec!(1073),
            calculate_coverage_points(indoor_cbrs).coverage_points
        );
    }

    #[test]
    fn outdoor_radios_consider_top_3_ranked_hexes() {
        let outdoor_wifi = RewardableRadio {
            radio_type: RadioType::OutdoorWifi,
            speedtests: Speedtest::best(),
            location_trust_scores: vec![MaxOneMultplier::from_f32_retain(1.0).unwrap()],
            verified_radio_threshold: true,
            hexes: vec![
                CoveredHex {
                    rank: Rank::new(1).unwrap(),
                    signal_level: SignalLevel::High,
                    assignments: Assignments::best(),
                    boosted: None,
                },
                CoveredHex {
                    rank: Rank::new(2).unwrap(),
                    signal_level: SignalLevel::High,
                    assignments: Assignments::best(),
                    boosted: None,
                },
                CoveredHex {
                    rank: Rank::new(3).unwrap(),
                    signal_level: SignalLevel::High,
                    assignments: Assignments::best(),
                    boosted: None,
                },
                CoveredHex {
                    rank: Rank::new(42).unwrap(),
                    signal_level: SignalLevel::High,
                    assignments: Assignments::best(),
                    boosted: None,
                },
            ],
        };

        // rank 1  :: 1.00 * 16 == 16
        // rank 2  :: 0.50 * 16 == 8
        // rank 3  :: 0.25 * 16 == 4
        // rank 42 :: 0.00 * 16 == 0
        assert_eq!(
            dec!(28),
            calculate_coverage_points(outdoor_wifi).coverage_points
        );
    }

    #[test]
    fn indoor_radios_only_consider_first_ranked_hexes() {
        let indoor_wifi = RewardableRadio {
            radio_type: RadioType::IndoorWifi,
            speedtests: Speedtest::best(),
            location_trust_scores: vec![MaxOneMultplier::from_f32_retain(1.0).unwrap()],
            verified_radio_threshold: true,
            hexes: vec![
                CoveredHex {
                    rank: Rank::new(1).unwrap(),
                    signal_level: SignalLevel::High,
                    assignments: Assignments::best(),
                    boosted: None,
                },
                CoveredHex {
                    rank: Rank::new(2).unwrap(),
                    signal_level: SignalLevel::High,
                    assignments: Assignments::best(),
                    boosted: None,
                },
                CoveredHex {
                    rank: Rank::new(42).unwrap(),
                    signal_level: SignalLevel::High,
                    assignments: Assignments::best(),
                    boosted: None,
                },
            ],
        };

        assert_eq!(
            dec!(400),
            calculate_coverage_points(indoor_wifi).coverage_points
        );
    }

    #[test]
    fn location_trust_score_multiplier() {
        // Location scores are averaged together
        let indoor_wifi = RewardableRadio {
            radio_type: RadioType::IndoorWifi,
            speedtests: Speedtest::best(),
            location_trust_scores: vec![
                MaxOneMultplier::from_f32_retain(0.1).unwrap(),
                MaxOneMultplier::from_f32_retain(0.2).unwrap(),
                MaxOneMultplier::from_f32_retain(0.3).unwrap(),
                MaxOneMultplier::from_f32_retain(0.4).unwrap(),
            ],
            verified_radio_threshold: true,
            hexes: vec![CoveredHex {
                rank: Rank::new(1).unwrap(),
                signal_level: SignalLevel::High,
                assignments: Assignments::best(),
                boosted: None,
            }],
        };

        // Location trust scores is 1/4
        assert_eq!(
            dec!(100),
            calculate_coverage_points(indoor_wifi).coverage_points
        );
    }

    #[test]
    fn boosted_hex() {
        let mut indoor_wifi = RewardableRadio {
            radio_type: RadioType::IndoorWifi,
            speedtests: Speedtest::best(),
            location_trust_scores: vec![MaxOneMultplier::from_f32_retain(1.0).unwrap()],
            verified_radio_threshold: true,
            hexes: vec![
                CoveredHex {
                    rank: Rank::new(1).unwrap(),
                    signal_level: SignalLevel::High,
                    assignments: Assignments::best(),
                    boosted: None,
                },
                CoveredHex {
                    rank: Rank::new(1).unwrap(),
                    signal_level: SignalLevel::Low,
                    assignments: Assignments::best(),
                    boosted: Multiplier::new(4),
                },
            ],
        };
        // The hex with a low signal_level is boosted to the same level as a
        // signal_level of High.
        assert_eq!(
            dec!(800),
            calculate_coverage_points(indoor_wifi.clone()).coverage_points
        );

        // When the radio is not verified for boosted rewards, the boost has no effect.
        indoor_wifi.verified_radio_threshold = false;
        assert_eq!(
            dec!(500),
            calculate_coverage_points(indoor_wifi).coverage_points
        );
    }

    #[test]
    fn base_radio_coverage_points() {
        let outdoor_cbrs = RewardableRadio {
            radio_type: RadioType::OutdoorCbrs,
            speedtests: Speedtest::best(),
            location_trust_scores: vec![MaxOneMultplier::from_f32_retain(1.0).unwrap()],
            verified_radio_threshold: true,
            hexes: vec![
                CoveredHex {
                    rank: Rank::new(1).unwrap(),
                    signal_level: SignalLevel::High,
                    assignments: Assignments::best(),
                    boosted: None,
                },
                CoveredHex {
                    rank: Rank::new(1).unwrap(),
                    signal_level: SignalLevel::Medium,
                    assignments: Assignments::best(),
                    boosted: None,
                },
                CoveredHex {
                    rank: Rank::new(1).unwrap(),
                    signal_level: SignalLevel::Low,
                    assignments: Assignments::best(),
                    boosted: None,
                },
                CoveredHex {
                    rank: Rank::new(1).unwrap(),
                    signal_level: SignalLevel::None,
                    assignments: Assignments::best(),
                    boosted: None,
                },
            ],
        };

        let indoor_cbrs = RewardableRadio {
            radio_type: RadioType::IndoorCbrs,
            speedtests: Speedtest::best(),
            location_trust_scores: vec![MaxOneMultplier::from_f32_retain(1.0).unwrap()],
            verified_radio_threshold: true,
            hexes: vec![
                CoveredHex {
                    rank: Rank::new(1).unwrap(),
                    signal_level: SignalLevel::High,
                    assignments: Assignments::best(),
                    boosted: None,
                },
                CoveredHex {
                    rank: Rank::new(1).unwrap(),
                    signal_level: SignalLevel::Low,
                    assignments: Assignments::best(),
                    boosted: None,
                },
            ],
        };

        let outdoor_wifi = RewardableRadio {
            radio_type: RadioType::OutdoorWifi,
            speedtests: Speedtest::best(),
            location_trust_scores: vec![MaxOneMultplier::from_f32_retain(1.0).unwrap()],
            verified_radio_threshold: true,
            hexes: vec![
                CoveredHex {
                    rank: Rank::new(1).unwrap(),
                    signal_level: SignalLevel::High,
                    assignments: Assignments::best(),
                    boosted: None,
                },
                CoveredHex {
                    rank: Rank::new(1).unwrap(),
                    signal_level: SignalLevel::Medium,
                    assignments: Assignments::best(),
                    boosted: None,
                },
                CoveredHex {
                    rank: Rank::new(1).unwrap(),
                    signal_level: SignalLevel::Low,
                    assignments: Assignments::best(),
                    boosted: None,
                },
                CoveredHex {
                    rank: Rank::new(1).unwrap(),
                    signal_level: SignalLevel::None,
                    assignments: Assignments::best(),
                    boosted: None,
                },
            ],
        };

        let indoor_wifi = RewardableRadio {
            radio_type: RadioType::IndoorWifi,
            speedtests: Speedtest::best(),
            location_trust_scores: vec![MaxOneMultplier::from_f32_retain(1.0).unwrap()],
            verified_radio_threshold: true,
            hexes: vec![
                CoveredHex {
                    rank: Rank::new(1).unwrap(),
                    signal_level: SignalLevel::High,
                    assignments: Assignments::best(),
                    boosted: None,
                },
                CoveredHex {
                    rank: Rank::new(1).unwrap(),
                    signal_level: SignalLevel::Low,
                    assignments: Assignments::best(),
                    boosted: None,
                },
            ],
        };

        // When each radio contains a hex of every applicable signal_level, and
        // multipliers are break even. These are the accumulated coverage points.
        assert_eq!(
            dec!(7),
            calculate_coverage_points(outdoor_cbrs).coverage_points
        );
        assert_eq!(
            dec!(125),
            calculate_coverage_points(indoor_cbrs).coverage_points
        );
        assert_eq!(
            dec!(28),
            calculate_coverage_points(outdoor_wifi).coverage_points
        );
        assert_eq!(
            dec!(500),
            calculate_coverage_points(indoor_wifi).coverage_points
        );
    }
}
