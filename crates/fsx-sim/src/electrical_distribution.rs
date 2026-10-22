//! Ideal closed-contact distribution for an explicitly supplied bus graph.
//! No FSX topology, consumer activity, impedance or parallel-source sharing is
//! inferred. Loads are explicit constant-current requests, not CFG maxima.
use fsx_vehicle::electrical::Bus;

#[derive(Debug, Clone, Copy)]
pub struct Contact {
    pub from: Bus,
    pub to: Bus,
    pub closed: Option<bool>,
}
#[derive(Debug, Clone, Copy)]
pub struct Feed {
    pub bus: Bus,
    /// A provider must explicitly establish source availability.
    pub online: Option<bool>,
    pub voltage: Option<f64>,
    pub capacity_amps: Option<f64>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Distribution {
    pub bus_voltage: [Option<f64>; 8],
    pub delivered_amps: [Option<f64>; 8],
    /// Corresponds to the caller's feed order; no equal-share rule is invented.
    pub feed_amps: Vec<Option<f64>>,
}
fn valid(value: Option<f64>, maximum: f64) -> bool {
    value.is_none_or(|v| v.is_finite() && (0. ..=maximum).contains(&v))
}
fn join(groups: &mut [usize; 8], a: usize, b: usize) {
    let old = groups[b];
    let new = groups[a];
    for group in groups {
        if *group == old {
            *group = new;
        }
    }
}

/// The caller supplies the complete source/contact inventory for these eight
/// buses. No feed means disconnected, not an omitted/unknown aircraft source.
/// Zero-resistance closed contacts are bidirectional; open contacts isolate.
/// Unknown contacts conservatively invalidate every possibly connected group.
pub fn solve(
    feeds: &[Feed],
    contacts: &[Contact],
    requested_amps: [Option<f64>; 8],
) -> Result<Distribution, String> {
    if feeds.len() > 8 || contacts.len() > 28 || requested_amps.iter().any(|&v| !valid(v, 1e6)) {
        return Err("Electrical distribution input budget/value exceeded".into());
    }
    let mut seen = [[false; 8]; 8];
    let mut groups = std::array::from_fn(|i| i);
    for contact in contacts {
        let a = contact.from.index()?;
        let b = contact.to.index()?;
        if a == b || seen[a][b] {
            return Err("Electrical contacts must be distinct, non-self bus pairs".into());
        }
        seen[a][b] = true;
        seen[b][a] = true;
        if contact.closed == Some(true) {
            join(&mut groups, a, b);
        }
    }
    for feed in feeds {
        feed.bus.index()?;
        if !valid(feed.voltage, 1000.) || !valid(feed.capacity_amps, 1e6) {
            return Err("Invalid electrical feed rating".into());
        }
    }
    // Unknown contacts only matter between groups not already connected through
    // known closed paths. Propagate uncertainty across chains of such contacts.
    let mut possible = groups;
    for contact in contacts.iter().filter(|c| c.closed.is_none()) {
        join(&mut possible, contact.from.index()?, contact.to.index()?);
    }
    let mut result = Distribution {
        bus_voltage: [None; 8],
        delivered_amps: [None; 8],
        feed_amps: feeds
            .iter()
            .map(|f| (f.online == Some(false)).then_some(0.))
            .collect(),
    };
    for root in 0..8 {
        if !groups.contains(&root) {
            continue;
        }
        let members: Vec<_> = (0..8).filter(|&i| groups[i] == root).collect();
        let uncertain_contact = members
            .iter()
            .any(|&i| (0..8).any(|j| possible[i] == possible[j] && groups[i] != groups[j]));
        if uncertain_contact {
            continue;
        }
        let candidates: Vec<_> = feeds
            .iter()
            .enumerate()
            .filter(|(_, f)| groups[f.bus.index().unwrap()] == root && f.online != Some(false))
            .collect();
        let total = members
            .iter()
            .try_fold(0., |sum, &i| requested_amps[i].map(|v| sum + v));
        let supply = match candidates.as_slice() {
            [] => Some(0.),
            [(index, feed)] if feed.online == Some(true) => {
                if feed.voltage == Some(0.) {
                    result.feed_amps[*index] = Some(0.);
                    Some(0.)
                } else if let (Some(total), Some(capacity), Some(voltage)) =
                    (total, feed.capacity_amps, feed.voltage)
                {
                    if total <= capacity {
                        result.feed_amps[*index] = Some(total);
                        Some(voltage)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            // Unknown availability and multiple active sources need a provider
            // or a parallel-source/charging model, never arbitrary priority.
            _ => None,
        };
        for i in members {
            result.bus_voltage[i] = supply;
            result.delivered_amps[i] = match supply {
                Some(0.) => Some(0.),
                Some(_) => requested_amps[i],
                None => None,
            };
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn feed() -> Feed {
        Feed {
            bus: Bus::Generator(1),
            online: Some(true),
            voltage: Some(28.),
            capacity_amps: Some(60.),
        }
    }
    fn link(from: Bus, to: Bus, closed: Option<bool>) -> Contact {
        Contact { from, to, closed }
    }
    #[test]
    fn closed_contacts_distribute_voltage_and_conserve_current_bidirectionally() {
        let contacts = [
            link(Bus::Main, Bus::Generator(1), Some(true)),
            link(Bus::Main, Bus::Avionics, Some(true)),
        ];
        let mut demand = [Some(0.); 8];
        demand[0] = Some(7.);
        demand[1] = Some(5.);
        let d = solve(&[feed()], &contacts, demand).unwrap();
        assert_eq!(
            d.bus_voltage,
            [
                Some(28.),
                Some(28.),
                Some(0.),
                Some(0.),
                Some(28.),
                Some(0.),
                Some(0.),
                Some(0.)
            ]
        );
        assert_eq!(d.feed_amps, vec![Some(12.)]);
        assert_eq!(d.delivered_amps[0], Some(7.));
        assert_eq!(d.delivered_amps[1], Some(5.));
        assert_eq!(d.delivered_amps.iter().flatten().sum::<f64>(), 12.);
        let mut open = contacts;
        open[1].closed = Some(false);
        let d = solve(&[feed()], &open, demand).unwrap();
        assert_eq!(d.bus_voltage[1], Some(0.));
        assert_eq!(d.delivered_amps[1], Some(0.));
        assert_eq!(d.feed_amps, vec![Some(7.)]);
    }
    #[test]
    fn unknown_contacts_propagate_but_redundant_unknown_paths_do_not_poison_closed_paths() {
        let mut contacts = vec![
            link(Bus::Main, Bus::Generator(1), Some(true)),
            link(Bus::Main, Bus::Avionics, None),
            link(Bus::Avionics, Bus::Battery, None),
        ];
        let d = solve(&[feed()], &contacts, [Some(0.); 8]).unwrap();
        for i in [0, 1, 2, 4] {
            assert_eq!(d.bus_voltage[i], None);
        }
        assert_eq!(d.bus_voltage[3], Some(0.));
        assert_eq!(d.feed_amps, vec![None]);
        contacts[1].closed = Some(true);
        contacts[2].closed = Some(true);
        contacts.push(link(Bus::Generator(1), Bus::Battery, None));
        let d = solve(&[feed()], &contacts, [Some(0.); 8]).unwrap();
        assert_eq!(d.bus_voltage[0], Some(28.));
    }
    #[test]
    fn offline_missing_ratings_unknown_load_and_overload_are_distinct() {
        let mut f = feed();
        let mut demand = [Some(0.); 8];
        demand[4] = Some(60.);
        assert_eq!(solve(&[f], &[], demand).unwrap().feed_amps, vec![Some(60.)]);
        demand[4] = Some(61.);
        let d = solve(&[f], &[], demand).unwrap();
        assert_eq!(d.bus_voltage[4], None);
        assert_eq!(d.feed_amps, vec![None]);
        demand[4] = None;
        assert_eq!(solve(&[f], &[], demand).unwrap().bus_voltage[4], None);
        f.online = Some(false);
        let d = solve(&[f], &[], demand).unwrap();
        assert_eq!(d.bus_voltage[4], Some(0.));
        assert_eq!(d.delivered_amps[4], Some(0.));
        assert_eq!(d.feed_amps, vec![Some(0.)]);
        f.online = None;
        assert_eq!(solve(&[f], &[], demand).unwrap().bus_voltage[4], None);
        f.online = Some(true);
        f.capacity_amps = None;
        demand[4] = Some(0.);
        assert_eq!(solve(&[f], &[], demand).unwrap().bus_voltage[4], None);
        f.voltage = Some(0.);
        assert_eq!(solve(&[f], &[], demand).unwrap().bus_voltage[4], Some(0.));
    }
    #[test]
    fn parallel_sources_never_invent_sharing_or_charging() {
        let mut other = feed();
        other.bus = Bus::Battery;
        other.voltage = Some(24.);
        let contacts = [link(Bus::Generator(1), Bus::Battery, Some(true))];
        let d = solve(&[feed(), other], &contacts, [Some(0.); 8]).unwrap();
        assert_eq!(d.bus_voltage[2], None);
        assert_eq!(d.feed_amps, vec![None, None]);
        other.online = Some(false);
        assert_eq!(
            solve(&[feed(), other], &contacts, [Some(0.); 8])
                .unwrap()
                .feed_amps,
            vec![Some(0.), Some(0.)]
        );
    }
    #[test]
    fn malformed_graphs_and_nonfinite_or_unbounded_ratings_fail() {
        for contacts in [
            vec![link(Bus::Main, Bus::Main, Some(true))],
            vec![
                link(Bus::Main, Bus::Battery, Some(true)),
                link(Bus::Battery, Bus::Main, Some(false)),
            ],
            vec![link(Bus::Main, Bus::Generator(0), None)],
        ] {
            assert!(solve(&[], &contacts, [Some(0.); 8]).is_err());
        }
        let mut f = feed();
        f.voltage = Some(f64::NAN);
        assert!(solve(&[f], &[], [Some(0.); 8]).is_err());
        f = feed();
        f.capacity_amps = Some(-1.);
        assert!(solve(&[f], &[], [Some(0.); 8]).is_err());
        assert!(solve(&[feed(); 9], &[], [Some(0.); 8]).is_err());
        for value in [-1., 1e6 + 1., f64::INFINITY] {
            let mut demand = [Some(0.); 8];
            demand[0] = Some(value);
            assert!(solve(&[], &[], demand).is_err());
        }
    }
}
