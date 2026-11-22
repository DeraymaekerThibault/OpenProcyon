# Battle sky: sphere and stars

Sources: the retail sky meshes and textures (`BD_Meshes/skybox_NN*.mdb`, `BD_Textures/*Skybox*`, `sky0NN*`), the render
effects `skybox.ret` and `3D_primative_vertexcolor_noz.ret`, and retail battle footage (Border Dispute, 1440x1080). Read from the
original, not measured: the star pattern is new every session, so there is no single value to check; the footage is the visual check.

## Order and switch

Each frame the battle background is drawn first: the sky sphere, then the stars. Both are drawn only while the Display
Options switch **Render Background** is on; it turns them off together.

## Sky sphere

- The map names one sky mesh (`Skybox mesh name`). It is a unit sphere, drawn around the camera with no turn or scale of its
  own and the camera's position removed: it turns with the view and never moves with the camera.
- Its texture is the original's 256x256 16-bit picture (`do_not_compress`, 5-6-5 bits per pixel), laid on the sphere by the
  mesh's own UVs: the picture repeats 1, 2 or 3 times around the horizon, and its height runs from the top pole to the bottom
  one (roughly v = (1 - height) / 2; eight skies use only v 0.127..0.875). The art is dark in its top and bottom rows, so
  looking far up or down shows dark sky; the battle camera normally looks across the bright band.
- Drawn unlit, vertex colour white, depth read on and depth write off.
- Here: the importer keeps the mesh's UVs as authored (`tools/mdb_to_glb.py`). Earlier imports squeezed every sphere's v range
  into the art's middle 40 % so no direction looked dark; that stretched the picture into streaks and is gone. The optional
  baked HD skies keep that squeeze as their own layout (`tools/bake_sky.py`, not retail).
- Open: the original's camera pitch range. In the reference footage the default camera is about 30 degrees above the plane;
  this engine's default orbit pitch is steeper (0.9 rad), so it sees more of the dark lower rows.

## Stars

The original generates the stars when the battle's world is built, from its cosmetic random generator (seeded from the clock
at start-up, not from the map), so every session shows a different pattern.

- Six groups, one colour each (sRGB, fully opaque):

  | group | stars | colour |
  |---|---|---|
  | 1 | 2500 | 0.75, 0.75, 0.75 |
  | 2 | 600 | 0.89, 0.909, 0.99 |
  | 3 | 600 | 0.804, 0.6, 0.6 |
  | 4 | 300 | 0.908, 0.909, 0.76 |
  | 5 | 1000 | 0.4235, 0.545, 0.557 |
  | 6 | 30 | 1, 1, 1 |

- Every star is a uniform random direction: height h = 2u - 1 (first draw), turn phi = 2 pi u (second draw), direction
  (cos phi sqrt(1 - h^2), sin phi sqrt(1 - h^2), h).
- A band of 1000 more white stars is then added to group 6, star i = 0..999 in turn: one draw picks one of the group's 30
  newest stars (the remainder of the generator's signed value by that count, made positive); a second draw u gives the
  distance u (i + 5) / 1000; the new star is that star plus the distance along a fresh random direction. Each star grows from
  the newest ones with a spread rising from 0.005 to about 1, so the band wanders across the sky.
- The generator: next = state x 1664525 + 1013904223 (32-bit); a draw u in [0, 1) is the low 23 bits as a fraction.
- Drawn as points exactly one pixel wide at any resolution, each group in its colour, with the camera's position removed (at
  infinity), right after the sky and with no depth test, so ships, scenery and effects drawn later all cover them.
- Here: `view::starfield` and `view::star_mesh`, a point list on the `Sky` entity just inside the sphere, seeded from the clock
  at each battle start (`scenario/spawn.rs`). A baked HD sky has its own stars, so the retail stars are drawn with the retail
  sphere only.
