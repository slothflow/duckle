import type { Edge, Node } from '@xyflow/react';
import type { DuckleNodeData } from '../pipeline-types';

// Node cards are min-width 220px and grow with longer labels, so leave a wide
// horizontal gap between dependency levels for the edges and side connectors to
// breathe, plus a generous vertical gap between siblings sharing a level.
const RANK_GAP_X = 460; // horizontal distance between dependency levels
const NODE_GAP_Y = 210; // vertical distance between nodes sharing a level
const ORIGIN_X = 80;
const ORIGIN_Y = 140;

/**
 * Arrange nodes left-to-right by dependency depth so the canvas reads in the
 * direction of the edges: roots on the left, every node one column to the right
 * of its deepest upstream, siblings stacked and vertically centered. Replaces
 * the old layout that ignored edges and flattened every node onto a single row
 * (issue #36).
 */
export function layoutByDependency(
    nodes: Node<DuckleNodeData>[],
    edges: Edge[],
): Node<DuckleNodeData>[] {
    if (nodes.length === 0) return nodes;

    const ids = new Set(nodes.map(n => n.id));
    const outgoing = new Map<string, string[]>();
    const indegree = new Map<string, number>();
    for (const n of nodes) {
        outgoing.set(n.id, []);
        indegree.set(n.id, 0);
    }
    for (const e of edges) {
        // Skip self-loops and edges whose endpoints aren't both on the canvas.
        if (!ids.has(e.source) || !ids.has(e.target) || e.source === e.target) continue;
        outgoing.get(e.source)!.push(e.target);
        indegree.set(e.target, (indegree.get(e.target) ?? 0) + 1);
    }

    // Kahn's algorithm, assigning each node the longest path length from any
    // root so a node always sits to the right of everything it depends on.
    const rank = new Map<string, number>();
    const remaining = new Map(indegree);
    const queue: string[] = [];
    for (const n of nodes) {
        if ((indegree.get(n.id) ?? 0) === 0) {
            rank.set(n.id, 0);
            queue.push(n.id);
        }
    }
    while (queue.length > 0) {
        const id = queue.shift()!;
        const r = rank.get(id) ?? 0;
        for (const t of outgoing.get(id) ?? []) {
            rank.set(t, Math.max(rank.get(t) ?? 0, r + 1));
            const left = (remaining.get(t) ?? 0) - 1;
            remaining.set(t, left);
            if (left === 0) queue.push(t);
        }
    }
    // A node left unranked is inside a cycle; park it one column past the
    // longest acyclic chain so it stays visible and out of the flow.
    let maxRank = 0;
    for (const r of rank.values()) maxRank = Math.max(maxRank, r);
    for (const n of nodes) {
        if (!rank.has(n.id)) rank.set(n.id, maxRank + 1);
    }

    // Group node ids by rank, preserving their original order for stability.
    const byRank = new Map<number, string[]>();
    for (const n of nodes) {
        const r = rank.get(n.id)!;
        if (!byRank.has(r)) byRank.set(r, []);
        byRank.get(r)!.push(n.id);
    }
    const indexInRank = new Map<string, number>();
    for (const list of byRank.values()) {
        list.forEach((id, i) => indexInRank.set(id, i));
    }

    return nodes.map(n => {
        const r = rank.get(n.id)!;
        const i = indexInRank.get(n.id)!;
        const count = byRank.get(r)!.length;
        return {
            ...n,
            position: {
                x: ORIGIN_X + r * RANK_GAP_X,
                y: ORIGIN_Y + (i - (count - 1) / 2) * NODE_GAP_Y,
            },
        };
    });
}
