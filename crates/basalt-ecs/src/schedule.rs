//! Dependency graph and parallel group computation for ECS systems.
//!
//! Builds execution groups from system component access declarations.
//! Systems within a group have no conflicting access and can run in
//! parallel. Groups execute sequentially with a barrier between them.
//!
//! The [`GroupCache`] precomputes groups for all tick offsets at startup,
//! making per-tick lookup O(1) instead of O(n²).

use basalt_core::{Phase, SystemDescriptor};

/// Greatest common divisor (Euclidean algorithm).
fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Least common multiple of two values.
fn lcm(a: u64, b: u64) -> u64 {
    if a == 0 || b == 0 {
        1
    } else {
        a / gcd(a, b) * b
    }
}

/// Precomputed execution groups for a specific phase.
///
/// Built once at startup from system access declarations. The groups
/// repeat on a cycle of `lcm(every₁, every₂, ..., everyₙ)` ticks.
/// Per-tick lookup is O(1) — just an index into the precomputed table.
pub(crate) struct GroupCache {
    /// Groups for each tick offset. Index = `tick % cycle_length`.
    groups_by_offset: Vec<Vec<Vec<usize>>>,
    /// The tick cycle length (LCM of all `every` values).
    cycle_length: u64,
}

impl GroupCache {
    /// Builds the cache by precomputing groups for every tick offset.
    ///
    /// The O(n²) graph coloring runs once per offset at startup.
    /// With typical `every` values {1, 5, 20}, the LCM is 20 —
    /// so we precompute 20 group configurations.
    pub fn build(systems: &[Option<SystemDescriptor>], phase: Phase) -> Self {
        let every_values: Vec<u64> = systems
            .iter()
            .filter_map(|s| s.as_ref())
            .filter(|s| s.phase == phase)
            .map(|s| s.every)
            .collect();

        if every_values.is_empty() {
            return Self {
                groups_by_offset: Vec::new(),
                cycle_length: 1,
            };
        }

        let cycle_length = every_values.iter().copied().fold(1u64, lcm);

        let groups_by_offset = (0..cycle_length)
            .map(|tick| compute_groups(systems, phase, tick))
            .collect();

        Self {
            groups_by_offset,
            cycle_length,
        }
    }

    /// Returns the precomputed groups for the given tick. O(1).
    pub fn groups_for_tick(&self, tick: u64) -> &[Vec<usize>] {
        if self.groups_by_offset.is_empty() {
            return &[];
        }
        let offset = (tick % self.cycle_length) as usize;
        &self.groups_by_offset[offset]
    }
}

/// Computes parallel execution groups for systems in a given phase.
///
/// Uses greedy graph coloring: systems are assigned to the first group
/// where they conflict with no existing member. If no such group exists,
/// a new group is created.
///
/// Returns groups of system indices (into the provided `systems` slice).
/// Groups are ordered: all systems in group N complete before group N+1 starts.
///
/// Only systems matching the given `phase` and whose `every` divides the
/// current `tick` are included.
fn compute_groups(
    systems: &[Option<SystemDescriptor>],
    phase: Phase,
    tick: u64,
) -> Vec<Vec<usize>> {
    // Collect indices of systems eligible this tick
    let eligible: Vec<usize> = systems
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            s.as_ref()
                .is_some_and(|s| s.phase == phase && tick.is_multiple_of(s.every))
        })
        .map(|(i, _)| i)
        .collect();

    if eligible.is_empty() {
        return Vec::new();
    }

    let mut groups: Vec<Vec<usize>> = Vec::new();

    for &sys_idx in &eligible {
        let sys_access = &systems[sys_idx].as_ref().unwrap().access;

        // Find the first group with no conflict
        let mut placed = false;
        for group in &mut groups {
            let conflicts = group.iter().any(|&existing_idx| {
                sys_access.conflicts_with(&systems[existing_idx].as_ref().unwrap().access)
            });
            if !conflicts {
                group.push(sys_idx);
                placed = true;
                break;
            }
        }

        if !placed {
            groups.push(vec![sys_idx]);
        }
    }

    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use basalt_core::SystemAccess;
    use std::any::TypeId;

    // Dummy component types for testing
    #[derive(Debug)]
    struct Position;
    impl basalt_core::Component for Position {}

    #[derive(Debug)]
    struct Velocity;
    impl basalt_core::Component for Velocity {}

    #[derive(Debug)]
    struct ParticleEffect;
    impl basalt_core::Component for ParticleEffect {}

    /// Helper to build a system slot with given access declarations.
    fn system_with_access(
        name: &str,
        reads: &[TypeId],
        writes: &[TypeId],
        every: u64,
    ) -> Option<SystemDescriptor> {
        let mut access = SystemAccess::new();
        for &r in reads {
            access.reads.insert(r);
        }
        for &w in writes {
            access.writes.insert(w);
        }
        Some(SystemDescriptor {
            name: name.to_string(),
            phase: Phase::Simulate,
            every,
            access,
            budget: None,
            runner: Box::new(|_: &mut dyn basalt_core::SystemContext| {}),
        })
    }

    #[test]
    fn empty_systems_produces_no_groups() {
        let systems: Vec<Option<SystemDescriptor>> = Vec::new();
        let groups = compute_groups(&systems, Phase::Simulate, 1);
        assert!(groups.is_empty());
    }

    #[test]
    fn single_system_produces_one_group() {
        let systems = vec![system_with_access(
            "physics",
            &[TypeId::of::<Position>()],
            &[TypeId::of::<Velocity>()],
            1,
        )];
        let groups = compute_groups(&systems, Phase::Simulate, 1);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0], vec![0]);
    }

    #[test]
    fn non_conflicting_systems_share_group() {
        let systems = vec![
            // Writes Position only
            system_with_access("a", &[], &[TypeId::of::<Position>()], 1),
            // Writes Velocity only — no conflict
            system_with_access("b", &[], &[TypeId::of::<Velocity>()], 1),
        ];
        let groups = compute_groups(&systems, Phase::Simulate, 1);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0], vec![0, 1]);
    }

    #[test]
    fn write_write_conflict_splits_groups() {
        let systems = vec![
            // Writes Velocity
            system_with_access("physics", &[], &[TypeId::of::<Velocity>()], 1),
            // Also writes Velocity — conflict
            system_with_access("ai", &[], &[TypeId::of::<Velocity>()], 1),
        ];
        let groups = compute_groups(&systems, Phase::Simulate, 1);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0], vec![0]);
        assert_eq!(groups[1], vec![1]);
    }

    #[test]
    fn write_read_conflict_splits_groups() {
        let systems = vec![
            // Writes Position
            system_with_access("physics", &[], &[TypeId::of::<Position>()], 1),
            // Reads Position — conflict (other writes what we read)
            system_with_access("particles", &[TypeId::of::<Position>()], &[], 1),
        ];
        let groups = compute_groups(&systems, Phase::Simulate, 1);
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn read_read_does_not_conflict() {
        let systems = vec![
            // Reads Position
            system_with_access("a", &[TypeId::of::<Position>()], &[], 1),
            // Also reads Position — no conflict
            system_with_access("b", &[TypeId::of::<Position>()], &[], 1),
        ];
        let groups = compute_groups(&systems, Phase::Simulate, 1);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0], vec![0, 1]);
    }

    #[test]
    fn three_systems_mixed_conflicts() {
        // physics: writes Position, Velocity
        // ai: writes Velocity — conflicts with physics
        // particles: reads Position, writes ParticleEffect — conflicts with physics, not ai
        let systems = vec![
            system_with_access(
                "physics",
                &[],
                &[TypeId::of::<Position>(), TypeId::of::<Velocity>()],
                1,
            ),
            system_with_access("ai", &[], &[TypeId::of::<Velocity>()], 1),
            system_with_access(
                "particles",
                &[TypeId::of::<Position>()],
                &[TypeId::of::<ParticleEffect>()],
                1,
            ),
        ];
        let groups = compute_groups(&systems, Phase::Simulate, 1);
        // physics in group 0, ai conflicts with physics → group 1
        // particles conflicts with physics (reads Position) → group 1
        // But particles and ai: ai writes Velocity, particles doesn't touch it → no conflict
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0], vec![0]);
        assert!(groups[1].contains(&1));
        assert!(groups[1].contains(&2));
    }

    #[test]
    fn filters_by_phase() {
        let systems = vec![
            system_with_access("simulate_sys", &[], &[TypeId::of::<Position>()], 1),
            Some(SystemDescriptor {
                name: "input_sys".to_string(),
                phase: Phase::Input,
                every: 1,
                access: SystemAccess::new(),
                budget: None,
                runner: Box::new(|_: &mut dyn basalt_core::SystemContext| {}),
            }),
        ];
        let groups = compute_groups(&systems, Phase::Simulate, 1);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0], vec![0]);
    }

    #[test]
    fn filters_by_tick_frequency() {
        let systems = vec![
            system_with_access("every_tick", &[], &[TypeId::of::<Position>()], 1),
            system_with_access("every_5th", &[], &[TypeId::of::<Velocity>()], 5),
        ];

        // Tick 1: only every_tick runs (1 % 5 != 0)
        let groups = compute_groups(&systems, Phase::Simulate, 1);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0], vec![0]);

        // Tick 5: both run
        let groups = compute_groups(&systems, Phase::Simulate, 5);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0], vec![0, 1]);
    }

    #[test]
    fn systems_with_no_access_declarations_share_group() {
        // Systems with empty access (no declared reads/writes) never conflict
        let systems = vec![
            system_with_access("a", &[], &[], 1),
            system_with_access("b", &[], &[], 1),
        ];
        let groups = compute_groups(&systems, Phase::Simulate, 1);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0], vec![0, 1]);
    }

    // -- GroupCache tests --

    #[test]
    fn cache_lookup_matches_direct_compute() {
        let systems = vec![
            system_with_access("every_tick", &[], &[TypeId::of::<Position>()], 1),
            system_with_access("every_5th", &[], &[TypeId::of::<Velocity>()], 5),
        ];
        let cache = GroupCache::build(&systems, Phase::Simulate);

        // Verify cache matches direct computation for several ticks
        for tick in 0..20 {
            let cached = cache.groups_for_tick(tick);
            let direct = compute_groups(&systems, Phase::Simulate, tick);
            assert_eq!(cached, &direct, "mismatch at tick {tick}");
        }
    }

    #[test]
    fn cache_cycle_length_is_lcm() {
        let systems = vec![
            system_with_access("a", &[], &[], 4),
            system_with_access("b", &[], &[], 6),
        ];
        let cache = GroupCache::build(&systems, Phase::Simulate);
        assert_eq!(cache.cycle_length, 12); // lcm(4, 6) = 12
    }

    #[test]
    fn cache_all_every_one_has_cycle_one() {
        let systems = vec![
            system_with_access("a", &[], &[TypeId::of::<Position>()], 1),
            system_with_access("b", &[], &[TypeId::of::<Velocity>()], 1),
        ];
        let cache = GroupCache::build(&systems, Phase::Simulate);
        assert_eq!(cache.cycle_length, 1);
        assert_eq!(cache.groups_by_offset.len(), 1);
    }

    #[test]
    fn cache_empty_systems() {
        let systems: Vec<Option<SystemDescriptor>> = Vec::new();
        let cache = GroupCache::build(&systems, Phase::Simulate);
        assert!(cache.groups_for_tick(0).is_empty());
        assert!(cache.groups_for_tick(99).is_empty());
    }

    #[test]
    fn cache_repeats_after_cycle() {
        let systems = vec![
            system_with_access("a", &[], &[TypeId::of::<Position>()], 1),
            system_with_access("b", &[], &[TypeId::of::<Velocity>()], 3),
        ];
        let cache = GroupCache::build(&systems, Phase::Simulate);
        assert_eq!(cache.cycle_length, 3);

        // Tick 0 and tick 3 should produce the same groups
        assert_eq!(cache.groups_for_tick(0), cache.groups_for_tick(3));
        assert_eq!(cache.groups_for_tick(1), cache.groups_for_tick(4));
        assert_eq!(cache.groups_for_tick(2), cache.groups_for_tick(5));
    }

    #[test]
    fn gcd_and_lcm_basic() {
        assert_eq!(gcd(12, 8), 4);
        assert_eq!(gcd(7, 3), 1);
        assert_eq!(gcd(0, 5), 5);
        assert_eq!(lcm(4, 6), 12);
        assert_eq!(lcm(1, 20), 20);
        assert_eq!(lcm(5, 5), 5);
    }
}
