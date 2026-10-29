## Adaptation Plan: Peridot -> Peridot

1. **Identity Swap**: Global replace "Peridot" $\rightarrow$ "Peridot".
2. **Backend Adaptation**:
   - Update `runtime/` (Rust) for SM8635 kernel nodes.
   - Update CPU frequency tables for POCO F6.
   - Map KGSL GPU profiles to Adreno 735.
3. **UI Update**:
   - Update `ui/flutter` and `branding/` assets.
   - Adapt Sconfig cards for peridot.
4. **Build System**:
   - Update `Cargo.toml` and Flutter package IDs.
