# Phases: checklist

Work top to bottom. A phase is done only when its acceptance tests pass and the results are pasted into its file.

- [ ] **Phase 1: Bare pipeline.** Hotkey → record → OpenAI STT → paste. *(Compiles and runs on Windows. Acceptance tests 1.4–1.15 still to run.)* → [phase-1.md](phase-1.md)
- [ ] **Phase 2: Cleanup + app styles + latency** → [phase-2.md](phase-2.md)
- [ ] **Phase 3: Pill overlay, tray, full settings app, onboarding (cloud path)** → [phase-3.md](phase-3.md)
- [ ] **Phase 4: Dictionary + snippets** → [phase-4.md](phase-4.md)
- [ ] **Phase 5: Command mode** → [phase-5.md](phase-5.md)
- [ ] **Phase 6: Polish: installer, auto-start, auto-update, signing** → [phase-6.md](phase-6.md)
- [ ] **Phase 7: More cloud providers + local models (+ onboarding local path)** → [phase-7.md](phase-7.md)
- [ ] **Phase 8: macOS** → [phase-8.md](phase-8.md)
- [ ] **Phase 9: Open-source release** → [phase-9.md](phase-9.md)

Phases 7 and 8 don't depend on 4–6, so they can be pulled forward if the owner wants local models or Mac sooner. Ask first.

Each phase file has the same sections: **Goal · Scope · Out of scope · Design refs · Acceptance tests · Results** (Phase 1 also has Tasks).
