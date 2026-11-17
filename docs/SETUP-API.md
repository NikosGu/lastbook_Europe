# Setup: what the window asks the engine

First-run setup and the app's **Library and local AI** screen are the same page (`web/setup.html`, `web/setup.js`). It
talks to the engine at `/api/setup/` (`crates/engine/src/setup/api.rs`), through the app's own address scheme, never a
network port. Every answer is JSON.

## Routes

| Request | Body | Answer |
|---|---|---|
| `GET /api/setup/state` | | `{mode, hardware, manifest, current, found, recommend, job}` (below) |
| `GET /api/setup/job` | | the job, or `null` |
| `POST /api/setup/start` | `{pack?, model? \| ai?, folder?, map?, address_id?, repair?}` | the new job |
| `POST /api/setup/pause`, `resume`, `cancel`, `retry` | | the job |
| `POST /api/setup/use` | `{path}` | a job using that library folder or file; for a map or model file before a library exists, `{job: null, noted: true}` |
| `POST /api/setup/finish` | | `{ok, folder}`; the app then starts again on the new library |
| `POST /api/setup/pick` | `{files?}` | the system's folder (or file) picker: `{path, free_bytes, disk}`, or `{path: null, cancelled: true}` |
| `POST /api/setup/delete-old` | `{path}` | deletes a library setup moved aside itself: `{ok, deleting}` |
| `POST /api/setup/remove-ai` | `{model?}` | removes the model file(s) from the open library: `{ok, freed_bytes}` |

`start` takes a pack id, a model id (`ai: true` asks for the largest that fits), a region code for the map (`map`) and for
address search (`address_id`, else the map's region), and a folder for a new library. In manage mode a request for what
is already there is skipped; `nothing` is answered when nothing is left to do.

**Errors** are `{error, code, params}`: one code for each sentence (`crates/engine/src/setup/api.rs`, `CODES`: for
example `too_deep` with `room`, `wrong_size` with `name`, `size_bytes` and `expected_bytes`, `not_library` with `name`),
the params it names, and the English sentence in `error`, for logs and as the fallback. Status 409 for `busy` and
`not_done`, 507 for `no_space` (with `need_bytes` and `free_bytes`, also at the top level), 413 for a body over 64 KB
(`request_too_large`, from the router), 400 otherwise (`bad_request` for a body that cannot be read). The window says each code in the person's language (`web/locales/*`: `setup.code.<code>`) and shows the
sentence only for a code it does not know.

## The state

- `mode`: `first-run` (no library yet) or `manage`.
- `hardware`: the system, `ram_bytes`, `unified_memory`, `gpus`, `disks` (`path`, `name`, `free_bytes`, `total_bytes`),
  and for the local AI `ai` (the suggested model: `fits`, `needs_bytes`, `estimate`, `why`, `model`, and `why_codes`, a
  list of `{code, params}` saying the same as `why`: `hardware.rs`, `AI_CODES`) and `ai_models` (the same for each model
  offered).
- `manifest`: what can be downloaded, without its addresses: `packs`, `maps`, `address_indexes`, `models`, each with
  `id`, `title`, `available`, sizes and files (`name`, `size`, `sha256`). Map and address-index ids are region codes
  (`LI`, `DE-HB`, `FR-ARA`…: ISO 3166-1 for a whole country, ISO 3166-2 first level for Germany, France, Italy, Spain and
  Poland), plus `europe` and `base`. Models add `family`, `params`, `active_params`, `ctx` and `languages`.
- `current`: the open library (`folder`, `pack`, `content_revision`, `map`, `map_bytes`, `address_indexes`, `models`,
  `old`), or `null`.
- `found`: libraries and files already on this computer that setup can use (`size_ok` when the size matches; the SHA-256
  is checked when one is used, and a file that does not match is left as it is).
- `recommend`: the suggested `pack`, `model`, `folder`, the room each choice needs, `why` in plain words, and
  `why_codes`, the same as a list of `{code, params}` (`api.rs`, `WHY_CODES`; the AI's reasons follow when no model fits).

## The job

`{id, state, items, error, started, folder}`. `state` is `waiting`, `downloading`, `verifying`, `unpacking`, `activating`,
`paused`, `done` or `failed`. Each item (`pack`, `map`, `addresses`, `model`) has its own `state`, `bytes_done`,
`bytes_total`, `rate_bps`, `eta_s`, `error`, and `source`: `found` (a file already here) or `download`.

The job is kept in the app's settings folder (`setup/job.json`), never beside a library, and a job found there after a
restart comes back paused. Downloads resume where they stopped (HTTP Range, with the ETag checked). A new library is
unpacked into `<library>.setup` beside the old one, checked, and switched in when the app next starts
(`setup/pending.json`); the old one is kept until the person deletes it.

## The network

Setup's downloader is the only code that connects: HTTPS on port 443 to the hosts in `crates/engine/src/dist.rs`, a
redirect only to another of them, no proxy from the system, and trust only in the roots built into the program
(`crates/engine/src/setup/roots.pem`). Every file is checked against its SHA-256 before it is used; zips are checked for
unsafe paths, links and sizes before anything is written.
