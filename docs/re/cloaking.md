# Cloaking  -- DRAFT 1 (data-derived; original pass pending)

Data: only `Ship_Procyon_Submersible` has `Is Cloakable Bool True` (ship custom info); all other 70+ ships False. Guns carry `Can Be Fired While Cloaked`: true only for `Gun_Torpedo`
and `Gun_Gattling_Light`; the submersible's own plasma gun `Gun_IC_PlasmaDualLight_Submersible` is False (so only its torpedoes fire while cloaked).
Rule action `Set Group/Unit Cloak State` (Group Name, Cloak State TRUE/FALSE, AI Can Override TRUE/FALSE); used only in mission 12 on group `ProcSubs` (5 submersibles: Wraith, Shadow, Shade, 2 unnamed).
Player order items `CLOAK_START/STOP` exist in the options menu (docs/re/docking-and-orders.md).
(more to follow)
