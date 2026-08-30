F = [(0,1),(0,2),(1,0),(1,1),(2,1)]
def norm(cs):
    mr, mc = min(r for r,_ in cs), min(c for _,c in cs)
    return tuple(sorted(((r-mr, c-mc) for r,c in cs)))
def rot(cs): return norm([(c,-r) for r,c in cs])
ROTS, cur = [], norm(F)
for _ in range(4):
    ROTS.append(cur); cur = rot(cur)
ROTS = sorted(set(ROTS))
print("F rotations (rotations only):", len(ROTS))

# From the search: 5x2 torus, piece 0 = orientation 1 anchored at its solve position,
# piece 1 = orientation 3. Re-derive anchors by replaying the same greedy rule.
P, Q = 5, 2
grid = [-1]*(P*Q); placements = []
def idx(r,c): return (r%P)*Q + (c%Q)
def solve(k):
    if -1 not in grid: return True
    s = grid.index(-1); r0, c0 = divmod(s, Q)
    for si, sh in enumerate(ROTS):
        ar, ac = sh[0]
        cells = [idx(r0-ar+r, c0-ac+c) for r,c in sh]
        if len(set(cells)) != 5 or any(grid[x]!=-1 for x in cells): continue
        for x in cells: grid[x] = k
        placements.append((si, r0-ar, c0-ac))
        if solve(k+1): return True
        for x in cells: grid[x] = -1
        placements.pop()
    return False
assert solve(0), "no tiling"
print("torus placements (orientation, anchor_row, anchor_col):", placements)

# Lift to the plane over a large region and verify.
R = 30
cover = {}
ok_shape = True
for tr in range(-4, R//P + 4):
    for tc in range(-4, R//Q + 4):
        for pi,(si, ar, ac) in enumerate(placements):
            cells = [(ar + tr*P + r, ac + tc*Q + c) for r,c in ROTS[si]]
            if norm(cells) not in ROTS: ok_shape = False
            for cell in cells:
                cover.setdefault(cell, []).append((tr,tc,pi))

region = [(r,c) for r in range(R) for c in range(R)]
counts = [len(cover.get(x, [])) for x in region]
print("every plane piece is a genuine F rotation:", ok_shape)
print("coverage over 30x30 interior -> min:", min(counts), "max:", max(counts))
print("VALID PLANE TILING:", ok_shape and min(counts)==1 and max(counts)==1)

# Membership lookup keyed on (row mod 5, col mod 2)
lut = {}
for r in range(R):
    for c in range(R):
        tr,tc,pi = cover[(r,c)][0]
        lut.setdefault((r%P, c%Q), set()).add(pi)
print("piece-id determined by (row mod 5, col mod 2)?",
      all(len(v)==1 for v in lut.values()))
print("lookup table:", {k: next(iter(v)) for k,v in sorted(lut.items())})

print("\n12x12 patch of the plane tiling (letter = piece instance):")
labels = {}
for r in range(12):
    row = ""
    for c in range(12):
        key = cover[(r,c)][0]
        labels.setdefault(key, chr(65+len(labels)%26))
        row += labels[key] + " "
    print("   ", row)
