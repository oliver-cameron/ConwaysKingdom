F=[(0,1),(0,2),(1,0),(1,1),(2,1)]
def norm(cs):
    mr,mc=min(r for r,_ in cs),min(c for _,c in cs)
    return tuple(sorted(((r-mr,c-mc) for r,c in cs)))
def rot(cs): return norm([(c,-r) for r,c in cs])
R=[]; cur=norm(F)
for _ in range(4): R.append(cur); cur=rot(cur)
R=sorted(set(R))
placements=[(1,0,-1),(3,2,0)]; P,Q=5,2
cover={}
for tr in range(-3,8):
    for tc in range(-3,14):
        for pi,(si,ar,ac) in enumerate(placements):
            key=(tr,tc,pi)
            for r,c in R[si]:
                cover[(ar+tr*P+r, ac+tc*Q+c)]=key
SYM="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789#@%&*+=<>?/|~"
seen={}
print("Plane tiling, 15 rows x 16 cols (each letter = one F-pentomino chunk):\n")
for r in range(15):
    line=""
    for c in range(16):
        k=cover[(r,c)]
        if k not in seen: seen[k]=SYM[len(seen)%len(SYM)]
        line+=seen[k]+" "
    print("   ",line)
print(f"\n  distinct chunks shown: {len(seen)}")
print("  the 4 rotations in use:")
for si in sorted({p[0] for p in placements}):
    cs=R[si]; h=max(r for r,_ in cs)+1; w=max(c for _,c in cs)+1
    print(f"    orientation {si}:")
    for r in range(h):
        print("      "+"".join("#" if (r,c) in cs else "." for c in range(w)))
