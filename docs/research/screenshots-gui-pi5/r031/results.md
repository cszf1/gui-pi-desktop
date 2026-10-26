# r031 round 1 (build-0c12ff8 / -testhooks, gate 0cae275)
- install: dl2 both tags sha256sum OK x4; dpkg -i testhooks deb; resources match; auth.json bypass removed
- self-check deb / AppImage / AppImage-testhooks: all ok, bundle 15053 match (B3, B4)
- s1 (deb, real): click Increment once -> exit 0, 75 s, gatelab Increment x1, final text correct, client_tool_unmarked 0 (B1, B9)
- e1 (bad model name): exit 4, timeline error "model request failed: 400 MODEL_NOT_AVAILABLE", stopped "ended with an error, the task was not done", status error (B2)
- a_to approve timeout: gate human_outcome timeout (person_burden no_answer), human.jsonl timed out, model: "nobody answered within 30 s" (B7). Timeline label still says "(refused)" -> round 2 wording
- a_close: gate window_closed, human.jsonl window closed. a_yes: accepted -> direct tier=human, file deleted. a_no: denied.

# r031 round 2 (2026-10-26) — fixes 49b621a + 4f661d5 (desktop), gate main 0cae275 unchanged
- N1 no-answer wording: timeline "person: no answer (timed out) - not a refusal; the step was not done", popup per-type countdown text, settings, notes to the model. PASS (a_to3, gto, r2-timeline-noanswer.jpg)
- N2 expired question no longer makes the next same one repeated: x2 repeated:false (x1 was true). PASS
- N3 popup height: 49b621a popup_fit had no effect (GTK non-resizable ignores resize); 4f661d5 resizable + min=max: approve 300, clarify 292, takeover 334/436, help 272, countdown visible. PASS on 2nd fix
- B5/B6/B10 PASS (r2-b5-main, r2-b5-rev, r2-b5b6, r2-b10-before/after)
- ra run_actions stops on Loading / new dialog PASS; gate 0cae275 tests vs bundled binary 130 OK
- stop x2 (pi+gui-gate+cua-driver gone, 0 actions after reset, new pgid) PASS; leak 0 hits; tokens_src client_usage PASS
- AppImage real task PASS (model made one extra click, reported honestly); t7 reviewer_allow PASS; t12 loop hints PASS
- tooling: watch.sh import -window hung Xvfb during tk2 (discarded); fixed to root crop via xwininfo
- full table: gui-pi-desktop docs/test-log.md section "2026-10-26 复测"
