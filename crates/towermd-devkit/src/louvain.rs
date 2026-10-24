//! Deterministic Louvain community detection for the cold start S3 step.
//!
//! Integer weights and integer gain comparisons (no floating point), nodes and
//! candidate communities visited in ascending index order, and a move only on a
//! strictly larger gain: the same graph and initial partition always yield the
//! same communities. The initial partition is the directory grouping, so a
//! node without edges never leaves its directory and the community count only
//! falls, never below `minimum`.
use std::collections::BTreeMap;

struct Level {
    /// Neighbour index to weight; self-loops are kept separately.
    adjacency: Vec<BTreeMap<usize, u64>>,
    loops: Vec<u64>,
}

impl Level {
    fn degree(&self, node: usize) -> i128 {
        2 * self.loops[node] as i128 + self.adjacency[node].values().map(|w| *w as i128).sum::<i128>()
    }
}

/// `edges` are undirected `(a, b, weight)`; `initial[i]` is node `i`'s starting
/// community. Returns each node's final community, named by one of the initial
/// community ids that survived.
pub fn communities(
    node_count: usize,
    edges: &[(usize, usize, u64)],
    initial: &[usize],
    minimum: usize,
) -> Vec<usize> {
    assert_eq!(initial.len(), node_count);
    let mut level = Level {
        adjacency: vec![BTreeMap::new(); node_count],
        loops: vec![0; node_count],
    };
    for &(a, b, weight) in edges {
        if weight == 0 {
            continue;
        }
        if a == b {
            level.loops[a] += weight;
        } else {
            *level.adjacency[a].entry(b).or_default() += weight;
            *level.adjacency[b].entry(a).or_default() += weight;
        }
    }
    // Original node -> node of the current level.
    let mut membership: Vec<usize> = (0..node_count).collect();
    // Current level node -> community id. Level 0 uses the initial ids.
    let mut partition: Vec<usize> = initial.to_vec();
    // Current level node -> the initial community id it stands for (level > 0).
    let mut labels: Vec<usize> = Vec::new();
    for depth in 0..16 {
        let moved = local_moving(&level, &mut partition, minimum);
        if depth > 0 && !moved {
            break;
        }
        // Aggregate: each non-empty community becomes one node of the next level.
        let mut communities: Vec<usize> = partition.clone();
        communities.sort_unstable();
        communities.dedup();
        let index: BTreeMap<usize, usize> = communities
            .iter()
            .enumerate()
            .map(|(position, community)| (*community, position))
            .collect();
        let mut next = Level {
            adjacency: vec![BTreeMap::new(); communities.len()],
            loops: vec![0; communities.len()],
        };
        for node in 0..level.loops.len() {
            let from = index[&partition[node]];
            next.loops[from] += level.loops[node];
            for (&neighbour, &weight) in &level.adjacency[node] {
                let to = index[&partition[neighbour]];
                if from != to {
                    *next.adjacency[from].entry(to).or_default() += weight;
                } else if node < neighbour {
                    // An internal edge is seen from both ends; count it once.
                    next.loops[from] += weight;
                }
            }
        }
        labels = communities
            .iter()
            .map(|community| if depth == 0 { *community } else { labels[*community] })
            .collect();
        for value in membership.iter_mut() {
            *value = index[&partition[*value]];
        }
        partition = (0..communities.len()).collect();
        level = next;
    }
    membership
        .into_iter()
        .map(|node| labels[partition[node]])
        .collect()
}

/// One Louvain local-moving phase. Returns whether any node moved.
fn local_moving(level: &Level, partition: &mut [usize], minimum: usize) -> bool {
    let degrees: Vec<i128> = (0..partition.len()).map(|node| level.degree(node)).collect();
    let total: i128 = degrees.iter().sum();
    if total == 0 {
        return false;
    }
    let mut sums: BTreeMap<usize, i128> = BTreeMap::new();
    let mut sizes: BTreeMap<usize, usize> = BTreeMap::new();
    for (node, community) in partition.iter().enumerate() {
        *sums.entry(*community).or_default() += degrees[node];
        *sizes.entry(*community).or_default() += 1;
    }
    let mut active = sizes.len();
    let mut any = false;
    for _ in 0..64 {
        let mut moved = false;
        for node in 0..partition.len() {
            let degree = degrees[node];
            if degree == 0 {
                continue;
            }
            let current = partition[node];
            if sizes[&current] == 1 && active <= minimum {
                continue;
            }
            let mut links: BTreeMap<usize, i128> = BTreeMap::new();
            for (&neighbour, &weight) in &level.adjacency[node] {
                *links.entry(partition[neighbour]).or_default() += weight as i128;
            }
            *sums.get_mut(&current).unwrap() -= degree;
            // Gain of joining community c, scaled by 2m²:
            // 2m·k_i,c − Σ_tot(c)·k_i.
            let gain = |community: usize, sums: &BTreeMap<usize, i128>| {
                links.get(&community).copied().unwrap_or(0) * total - sums[&community] * degree
            };
            let mut best = current;
            let mut best_gain = gain(current, &sums);
            for &community in links.keys() {
                if community == current {
                    continue;
                }
                let candidate = gain(community, &sums);
                if candidate > best_gain {
                    best = community;
                    best_gain = candidate;
                }
            }
            *sums.get_mut(&best).unwrap() += degree;
            if best != current {
                *sizes.get_mut(&current).unwrap() -= 1;
                *sizes.get_mut(&best).unwrap() += 1;
                if sizes[&current] == 0 {
                    active -= 1;
                }
                partition[node] = best;
                moved = true;
                any = true;
            }
        }
        if !moved {
            break;
        }
    }
    any
}

#[cfg(test)]
mod tests {
    use super::communities;

    #[test]
    fn isolated_nodes_keep_their_directory() {
        assert_eq!(communities(3, &[], &[0, 1, 2], 3), vec![0, 1, 2]);
    }

    #[test]
    fn a_file_joins_the_community_it_imports_from() {
        // 0,1 in A; 2,3 in B; 4 sits in B but only talks to A.
        let edges = [(0, 1, 3), (2, 3, 3), (4, 0, 2), (4, 1, 2), (5, 6, 1)];
        let result = communities(7, &edges, &[0, 0, 1, 1, 1, 2, 2], 3);
        assert_eq!(result[4], 0);
        assert_eq!(result[2], result[3]);
        assert_eq!(communities(7, &edges, &[0, 0, 1, 1, 1, 2, 2], 3), result);
    }

    #[test]
    fn never_merges_below_the_minimum() {
        let edges = [(0, 1, 5), (1, 2, 5), (0, 2, 5)];
        let result = communities(3, &edges, &[0, 1, 2], 3);
        assert_eq!(result, vec![0, 1, 2]);
    }
}
