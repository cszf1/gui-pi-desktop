---
name: gate-replies
description: What each "[gate]" reply means and the right reaction (look again, re-aim, loop breaker, risk refusal, person). Load when a step comes back "Not executed".
---
# Gate replies
| Reply says | Meaning | Do |
|---|---|---|
| "fresh tree is attached" | target unclear / stale / new window | choose the target again from the attached tree; resend |
| "fresh screenshot ... attached" | x/y aim is out of date | re-aim on the attached screenshot; resend |
| "token was stale; re-bound" | the gate fixed your token | nothing; it ran |
| "reviewer doubts this step" | the gate's reviewer thinks the target is wrong | check the attached tree; choose another target or confirm the same one deliberately |
| "Loop breaker (level N/3)" | this way keeps failing | switch mode: keyboard, menu, another element, run_command |
| "this exact step already made no progress" (stop_loop) | same as above, harder | never resend it |
| "A person did this step themselves" | the person did it | do not repeat it; continue from the current window |
| "No person took this step" | the person declined to do it | try a clearly different way; follow their hint if given |
| "the person declined" / "did not answer within" | refused | take the safe way named; do not retry the effect |
| "the same effect was already refused" | you retried a refused effect | stop trying it; finish another way or report |
| "send it again ON ITS OWN" | a step in run_actions needs a person | resend that single step outside run_actions |
| "stop here and tell the user" | too many refusals | stop; explain what you wanted to do and why |
