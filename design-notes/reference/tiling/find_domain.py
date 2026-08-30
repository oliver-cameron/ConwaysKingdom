import sys
from itertools import product

F = [(0,1),(0,2),(1,0),(1,1),(2,1)]

def norm(cs):
    mr, mc = min(r for r,_ in cs), min(c for _,c in cs)
    return tuple(sorted(((r-mr, c-mc) for r,c in cs)))

def rot(cs):                       # 90 deg CW
    return norm([(c, -r) for r,c in cs])

def mirror(cs):
    return norm([(r, -c) for r,c in cs])

def orients(base, reflections):
    out, cur = set(), norm(base)
    for _ in range(4):
        out.add(cur)
        if reflections: out.add(mirror(cur))
        cur = rot(cur)
    return sorted(out)

def tile_torus(P, Q, shapes):
    """Tile the P x Q torus. A solution == a plane tiling periodic in (P,0),(0,Q)."""
    n = P*Q
    grid = [-1]*n
    placement = {}
    def idx(r,c): return (r % P)*Q + (c % Q)
    def solve(k):
        # first empty cell, row-major
        try: start = grid.index(-1)
        except ValueError: return True
        r0, c0 = divmod(start, Q)
        for si, sh in enumerate(shapes):
            # anchor: the shape's own first cell in row-major order sits on (r0,c0)
            ar, ac = sh[0]
            cells = [idx(r0 - ar + r, c0 - ac + c) for r,c in sh]
            if len(set(cells)) != 5: continue          # self-overlap by wraparound
            if any(grid[x] != -1 for x in cells): continue
            for x in cells: grid[x] = k
            placement[k] = (si, (r0-ar, c0-ac))
            if solve(k+1): return True
            for x in cells: grid[x] = -1
            del placement[k]
        return False
    return (grid, placement) if solve(0) else None

for label, refl in (("rotations only", False), ("rotations + reflections", True)):
    shapes = orients(F, refl)
    print(f"=== {label}: {len(shapes)} distinct orientations ===")
    found = []
    for total in range(5, 101, 5):
        for P in range(1, total+1):
            if total % P: continue
            Q = total // P
            if P > 12 or Q > 12: continue
            res = tile_torus(P, Q, shapes)
            if res: found.append((P,Q,res))
        if found: break
    if not found:
        print("  no fundamental domain found within search bounds\n"); continue
    P,Q,(grid,pl) = found[0]
    print(f"  smallest translational fundamental domain: {P} x {Q} = {P*Q} cells, {P*Q//5} pentominoes")
    if len(found) > 1:
        print("  others at same area:", ", ".join(f"{p}x{q}" for p,q,_ in found[1:]))
    for r in range(P):
        print("   ", " ".join(f"{grid[r*Q+c]:2d}" for c in range(Q)))
    print("  piece -> orientation index:", {k: v[0] for k,v in sorted(pl.items())})
    print()
