# Plugin input and capability review

Reviewed the six recommended plugins' operation entry points, manifest capabilities and host integration boundaries. This is a scoped implementation review and regression suite, not an independent audit or fuzzing proof.

| Plugin | Privileged permission / declared interfaces used | Checked boundaries |
| --- | --- | --- |
| To-do | none / Notes | Required title <=500 chars; priority 0–3; allowlisted repeats; calendar dates; explicit ASCII HH:MM reminders; parameterized storage. Hardened reminder validation rejects signed numeric strings such as `+1:00`. |
| Bookmarks | none / none | HTTP(S) only, bounded URLs/titles/notes/tags, invalid schemes/control characters rejected; opening is delegated to the host rather than plugin network access. |
| Notes | none / Tasks | Body <=200,000 chars, title/project limits, text merge and parameterized queries; task conversion uses declared interface operations. |
| Habits | none / none | Bounded text, whole-number goals, weekdays strictly 0–6, ASCII HH:MM reminder validation, notification-target allowlist. |
| Voice | speech / Tasks, Notes | <=20,000-char text, streaming base64 decode <=8 MiB, audio magic/MIME checks; queued audio loaded individually; successful transcription clears the active audio field. Historical CRDT/backup copies remain. |
| Reports | AI / Tasks, Habits, Notes | Date/range/period limits, prompt/reflection/skill text limits, selected AI sources/private-task exclusions; schedule fields now reject malformed time/day/week inputs instead of silently substituting or filtering them. |

Host integration tests send hostile values to all six actual WASM components and verify usable operations still succeed. Host fixtures additionally return an oversized success payload after a write, or exceed the aggregate write budget; both must fail at the intended boundary and roll back partial writes. Existing capability/interop/SQL/fuel/memory tests remain required.

Still open: full field-type strictness (some optional form fields intentionally coerce/ignore malformed values), parser fuzzing, whole-device storage quotas, every callback/error/notification preview path, hostile same-user filesystem races and physical-device performance. Reports source filters are a product privacy feature, not a prompt-injection security boundary. Model text cannot execute plugin/native code through the declarative renderer. Plugin WASM does not get arbitrary network/filesystem imports; permitted host AI/speech/interop calls remain host-enforced.

Source manifests/capabilities must be reviewed again when changed; no permissions or interfaces were expanded by this milestone. Uncommitted To-do board/matrix feature changes were preserved separately and are excluded from the security commit.
