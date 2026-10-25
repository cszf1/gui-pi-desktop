---
name: gate-replies
description: What each specific "[gate]" reply and "[client: ...]" note means and the right reaction (fresh tree, re-aim, loop breaker, reviewer, refused effect, batch). Load when a step comes back "Not executed" or with a note you are unsure about.
---
# Gate replies
| Reply says | Meaning | Do |
|---|---|---|
| "fresh tree is attached" | target unclear / stale / new window | choose the target again from the attached tree; resend |
| "fresh screenshot ... attached" | x/y aim is out of date | re-aim on the attached screenshot; resend |
| "token was stale; re-bound" | the gate fixed your token | nothing; it ran |
| "reviewer doubts this step" | the gate's reviewer thinks the target is wrong | check the attached tree; choose another target. You cannot overrule the reviewer |
| "Loop breaker (level N/3)" / "try a clearly different way" | this way keeps failing | switch mode: keyboard, menu, another element, run_command |
| "this exact step already made no progress" (stop_loop) | same, harder | never resend it |
| client note "RETHINK" | the gate wants a new candidate, not this one | reconsider from the current window; generate a different next action |
| "No person took this step" | the person declined to do it | try a clearly different way; follow their hint if given |
| "the same effect was already refused" | you retried a refused effect | stop trying it; finish another way or report |
| a risky step refused with a safe way named | the gate will not run it | take the safe way and finish the task |
| "send it again ON ITS OWN" | a step inside run_actions needs a person | resend that single step outside run_actions |
| "stop here and tell the user" | too many refusals | stop; explain what you wanted to do and why |
| client note "diff against snapshot X, which is no longer in context" | the base tree was dropped | read the window before using any token not shown |
