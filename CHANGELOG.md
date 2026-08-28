# Changelog

All notable changes to Your Own AI are documented here. The release workflow
extracts the entry matching the pushed tag into the GitHub release notes.

## [0.6.1] - 2026-08-28

### Models
- Nemotron 3.5 Lightning loads again. The maker replaced its download after
  our catalog pinned it, with a copy no current engine can read; the catalog
  now points at the copy that loads, and the Offline Models page says
  "Re-download needed" on a file the maker has replaced, instead of a green
  badge, with delete and download one click away.
- A model file the engine cannot read is now said in those words - in chat
  and on its card - and never as "too large for your graphics card".
- A download finished from an earlier session now fetches the model's
  speed-up file like a fresh download does, instead of leaving it as a link.

### Macs and engines
- The free-memory check reads memory the way macOS means it (free plus
  inactive and purgeable pages), sizes the file that will actually load,
  re-samples once before refusing, and no longer tells you to pick a
  smaller model. A settled Mac with plenty of headroom was refused a 2 GB
  model for as long as it stayed settled.
- A send that was refused no longer piles up unanswered turns for the next
  one: consecutive user turns go to the engine as one message (your
  records keep each as it happened). Strict chat templates rejected the
  pile, and the error blamed the model.
- An engine that stops answering is given ten minutes, then the reply is
  marked failed and the next send starts a fresh server; an MLX server that
  rejects a request is restarted before the next turn instead of wedging.
- The GPU safety net no longer counts a reboot, or a run that never reached
  the graphics card, as a crash. Four of those put a Mac on CPU.
- Engine changes apply now, not next launch: getting or removing a model's
  MLX version, removing the MLX engine, or installing the CUDA engine
  reloads the model you have open onto the engine that now applies.
- Speed is one number with one meaning on every engine - completion tokens
  over the time they streamed - and the tokens panel also shows the
  reading speed when the engine reports it. A model's card keeps separate
  measured speeds for MLX and the standard engine, and says which one
  chats are using.
- "Try GPU again" tries it now: the open model reloads on the graphics
  card at once instead of at the next launch.
- MLX preview: SwiftLM b700 - text-only Qwen 3.5 and 3.6 mixture-of-experts
  models are no longer mistaken for vision models.

### Projects
- An agent that opens a picture or a PDF no longer loses the turn to an
  engine error when its model has no vision. In an automatic mode the call
  goes to a downloaded model that can see, when one fits; otherwise the
  image becomes a note the AI can act on (a PDF can be read as text), and
  the rail says why the picture was not looked at - a line that stays
  readable after the steps fold and is kept in your records.

### Flowsta Vault - the path to backups and online models
- Settings and the Online Models page now tell the same story before the
  click: what the Vault is (your identity on your own computer, no
  password, nobody can lock you out), what it does for you here (backups
  with one-click recovery, signed exports, online models the most private
  way there is), and what it takes (free, about two minutes, 24 recovery
  words to keep on paper). "Get Flowsta Vault" opens the handoff page that
  says to come back to Your Own AI - both pages notice the moment the
  Vault is ready. "Pro feature" is gone; it is the optional plan.

### First run
- The welcome offers one model, the one graded best for this computer, and
  nothing that leads away from it. Close the app mid-download and the
  welcome picks the download up where it stopped the next time, never
  from zero; a download still running when you come back is joined, not
  restarted.
- Honest words for small machines: a processor-only pick says "good for
  everyday chat, slower on long answers"; a machine below what the
  smallest model needs is told so, in amber, with the optional online plan
  named as the fast route. The old "fast and efficient on your CPU" line
  is gone.
- The first question after a model loads pays only for its own words: the
  AI's instructions are read into the engine right after the load. While
  the model reads a question, the bubble says so after a few seconds
  instead of a generic thinking line.

### Projects
- Checks after edits. When the agent thinks a turn is done, the project's
  own check command runs first - a typecheck, check or lint script from
  package.json, cargo check, go vet, or ruff - and failures go straight
  back to the agent to fix before it finishes (two rounds at most). The
  outcome is written on the turn: "Checks passed (npm run typecheck)" or
  the errors that remain. Projects without a check command are left
  alone.
- Undo this turn. A finished turn that changed files offers "Undo this
  turn's changes" under its steps: every edit is put back, files the turn
  created are removed, files it deleted are restored - in any folder, with
  or without git. What was undone is written on the turn and kept in your
  records. This is what makes "approve everything" safe to pick.
- Reading a project's memory no longer holds up the agent's turn. The
  current memory is kept ready (and in an encrypted copy on disk for the
  next start); a new note updates it in place instead of throwing it away,
  and the records are re-checked in the background. A read that used to
  walk every memory revision - close to a minute on a busy machine - now
  answers at once.

### Add-ons
- Share it with everyone. An AI's Export dialog is now two choices, Share
  publicly or Export to file, both signed with your Flowsta name (the Vault
  status sits under them); a skill's card offers the same share. Both go to
  the public add-ons directory, and the app shows where a share got to
  (checking, waiting for a look, listed, or not added with the note left
  for you) on the AI's edit form and the skill's card: the app signs the pack or
  the skill folder with your Flowsta identity and files it for review
  under your Flowsta name - no GitHub account, no upload site. Once it is
  on the shelf, everyone's Characters and Skills pages list it, with your
  name on it. Free listings for now.
- The Characters shelf and the Recommended skills read the directory when
  it answers, so shared add-ons appear without an app update; the built-in
  lists remain the fallback offline.
- Skills. A skill is a folder of instructions an AI reads when the work
  calls for it (the Agent Skills open standard: SKILL.md plus supporting
  files). Add-ons > Skills lists what is installed and adds more from a
  folder, a zip file, or a link - a GitHub repository is pinned to the
  commit it points at today. Skills work in projects (the AI picks one by
  its description) and in chat, where the AI carries a short list of its
  chosen skills and the full text of the one that fits the question, so a
  few skills stay cheap (each card shows what a skill costs when used). A
  skill does nothing until you give it to an AI - "Used by" on its card,
  or Your AIs > edit > Skills; a freshly added skill opens that picker. A
  skill is text - adding one never runs anything; one that ships programs
  says so.
- Proof on the turn: a chat reply's model details now list the skills
  that rode with it, and a project step that reads inside a skill says
  "Using skill: name" instead of "Reading SKILL.md".
- Each skill card can show its SKILL.md, says when the source branch has
  moved on ("Update available") with a one-tap update, and lets you choose
  which AIs use the skill from the card itself.
- A Recommended list inside "Add a skill": open-standard skills we have
  read and pinned (Holochain development, debugging, incremental
  implementation, decision records, front-end design, refining an idea,
  internal communications, a second look before you act) - knowledge
  only, permissive licenses, sized for a local model. One tap installs
  exactly the reviewed version.
- A project that brings its own skills (.claude, .agents or .grok skills
  folders) shows "N skills" on its pill in the header.
- yourownai:// links open the app at the matching page; a skills link
  arrives with the source filled in, ready to add.
- Characters. The eight from yourownai.net on one shelf - complete AIs
  with a personality, a portrait and starting memories, signed by Flowsta.
  "Make it mine" creates the AI here, the same way Import AI does, and it
  lives in Your AIs from then on; the shelf stays for the next one.
- Add-ons has its own place in the menu: Projects, Skills, Characters, and
  the kinds that follow.


## [0.6.0] - 2026-08-25

Your AIs know you better, run bigger models on the machine you already
have, and choose between them by what actually runs well on it.

### Highlights

- **Your AIs know you as a whole, not as a list.** From what they have
  learned, they keep a short summary of who you are - "How your AIs see
  you" - written on your device and rewritten as things change. You can
  read it any time, and if it is wrong you fix the fact it came from and
  it rewrites itself. Every fact is yours to edit or forget, traceable to
  the conversation it came from, and none of it leaves your machine. See
  *Memory*.
- **Bigger models on the machine you already have.** Mixture-of-experts
  models split between your graphics card and main memory - measured 4.5x
  faster on an 8 GB card - and the catalog now has a mixture-of-experts
  for every size of machine, from a 4.8 GB file for 12 GB laptops to
  753B for workstations. See *Big mixture-of-experts models* and *New
  models*.
- **Routing that measures, not assumes.** On your device, the app times
  how fast each model actually runs on this computer as you use it, and
  picks by those numbers - not a spec sheet. Online models follow your
  preferences instead. See *Routing*.
- **It fails politely.** The first-run recommendation sizes to your whole
  machine, a load that cannot succeed is refused with a plain sentence,
  and the app never crash-loops on a model that took it down. See *The
  app fails politely*.
- **Models live where you say.** Store models on any drive, with free
  space checked before every download. See *Model storage*.
- **Records that tell the truth.** A conversation is in your records the
  moment you send; a stopped reply is kept and marked. See *Your
  records*.
- **Apple Silicon: MLX engine, as a preview.** Optional, per model, chats
  only, no speed promise. See *Apple Silicon*.

### A damaged model file can't take the app down

- A model file that is incomplete or corrupted (a browser download that
  stopped early, a copy that went wrong) could crash the app at startup
  while it looked through your models folder. Every model header is now
  read within hard bounds; such a file is listed as "Damaged file" with a
  delete button, is never loaded or offered, and downloading the same
  model again replaces it.
- A finished download is checked to read as a model before it gets its
  final name.

### Big mixture-of-experts models run on small graphics cards

- A mixture-of-experts model bigger than your graphics memory now runs
  with its rarely-used parts in main memory and the rest on the card,
  instead of being refused or crawling. Measured on an 8 GB RTX 4060 Ti
  with 32 GB of RAM: Qwen3.6 35B-A3B went from 6 to 26 tokens per second,
  gpt-oss 20B from 14 to 23, and the 35B loads in seconds instead of most
  of a minute. The gate is main memory: a 32 GB machine qualifies for the
  35B, a 16 GB machine is told so honestly. The split is sized from the
  model file itself - as many of the model's expert layers as fit stay on
  the card, the rest go to main memory - so a bigger card keeps more.
  Model cards say "Runs here - split with main memory", and "Best for
  this computer" can recommend these models.
- Downloaded models show this computer's measured generation speed
  ("~27 tok/s measured") once you have used them - your number, not a
  benchmark's.
- A reply's tokens-per-second now comes from the local engine's own
  timing, so a short one-line answer no longer reads as tens of
  thousands of tokens per second.
- Models that always think before answering (LFM2.5) now answer directly
  in chat and think in Report mode, so a short question never ends inside
  the model's reasoning with nothing visible. gpt-oss replies no longer
  stop at the end of its reasoning: one of the app's generic stop markers
  was the same token gpt-oss uses to close its reasoning channel, which
  cut every reply off before the answer. gpt-oss is no longer used for
  memory extraction, which it cannot format.

### New models - mixture-of-experts for every size of machine

- Two workstation-class additions for very large machines: **DeepSeek V4
  Flash** (284B mixture-of-experts, 13B active, 1M context; 128 GB+ of
  main memory) and **GLM-5.2** (753B mixture-of-experts; 384 GB+). Both
  ship as multi-part files: the app downloads the parts in order (each
  part resumes on its own), shows "part 2 of 3", treats the set as one
  model, and keeps them off machines that cannot run them.
- Every model file's layout is now checked against its size when it is
  read, so a download cut off inside the weights - not just in the header
  - shows as damaged instead of failing to load.
- Where the maker ships a speed-up file for a model (Nemotron 3.5
  Lightning's multi-token head, DeepSeek V4 Flash's DSpark draft), the
  app downloads it after the model, keeps it beside it, and the engine
  uses it to answer faster - several drafted tokens checked per pass,
  which matters most when a model's experts live in main memory. A model
  downloaded earlier offers "Get its speed-up file" on the Models page.
- The split between graphics card and main memory learns from your
  machine: after a model loads, the app measures what the card actually
  held and uses that for the next load, so it settles on the most the
  card can carry. If a split ever runs out of graphics memory, the app
  retries at once with the experts in main memory instead of reporting
  the model too large.

- Four additions to the offline catalog, all mixture-of-experts and all
  running on the new split-memory path: **LFM2.5 8B-A1B** (Liquid AI -
  a 4.8 GB file for 12-16 GB machines, the first MoE that fits them),
  **Granite 4.0 Tiny** (IBM - 3.9 GB, a 1M-token context, Apache-2.0),
  **Nemotron 3.5 Lightning 30B-A3B** (NVIDIA's August 2026 reasoning
  model, for 32 GB machines) and **Ling-mini 2.0** (Ant Group - 16B-A1.4B,
  MIT, for 24 GB machines). LFM2.5 asks you to accept Liquid AI's license
  once before its first download.

### Apple Silicon: MLX engine (preview)
- Macs with Apple Silicon can add an optional MLX engine (Settings >
  Engines, about 50 MB, checked against a pinned release before it runs)
  and then fetch MLX versions of supported models - 15 in the catalog,
  from Ministral 3B to Qwen 3.6 35B - with "Get the MLX version" on a
  downloaded model's row, and an MLX chip on catalog cards once the engine
  is installed. Chats run on MLX once a model's MLX version is installed;
  project work, images, memory and everything else stay on the standard
  engine, and your existing model file stays. Whether MLX is faster
  depends on your Mac - the app makes no promise, and nothing changes
  unless you install it. Remove on the engine card puts everything back.
- MLX downloads resume file by file, show live progress, and pick back up
  after navigating away or restarting the app.

### Model storage
- Choose where models are stored - and not just chat models: vision files,
  speed-up drafts, engine packs, OCR models, and the Build agent all live in
  the same folder and move together. The control is on the offline models
  page and in Settings > Components. It shows how much is stored and how
  much room the drive has, with a Change button that moves everything to a
  folder you pick - a second drive, an external SSD. Downloads follow, moves
  roll back safely if interrupted, and if the drive is missing later the app
  falls back to its default folder until it returns.
- Downloads check disk space up front and say plainly what is missing
  ("needs 22.4 GB free on the models drive, 9.1 GB available") instead of
  failing mid-file with "no space left".

### Downloads
- A resumed download now proves it is continuing the same file it started
  (If-Range): if the model was re-uploaded upstream in between, the download
  starts over instead of stitching two versions into one broken file.
- A model file the engine rejects ("layout doesn't match") is reported in
  those words with the fix (delete and re-download), sits out the session,
  and no longer counts against the graphics card - two rejections of one
  bad file could previously switch the whole app off CUDA.
- If CUDA was ever switched off by the safety ladder, the notice offering
  "Try GPU again" now stays available instead of appearing only once.

- Several models can download at once, each card with its own progress
  and button. Starting a second download used to make the first card look
  idle.
- Coming back to the Models page shows a download's real progress right
  away, and a download the app was closed in the middle of picks itself
  back up from where it stopped.
- A vision model is two files; the card now says which one is in flight
  ("Downloading model · 1 of 2", then "Downloading vision support · 2 of
  2"), shows downloaded of total, and the Download button states both
  sizes up front ("3.2 GB + 0.9 GB vision") - the bar no longer looks
  like it started over.

### Routing
- The GPU + RAM split now checks how much of a mixture-of-experts model
  actually fires on every token. Catalog models activate a few percent -
  their experts are cold mass and the split is fast. A converted
  "surgery" model that activates most of its width gets the honest
  "Runs slower" grade instead of a promise the hardware can't keep
  (it still runs if you pick it).
- Project work: local picks for agent sessions rank tool capability first (the speed lean only decides between equals); a local model counts as ready for agent work only when it fits fast and, once measured here, keeps 8+ tokens a second - the same bar for side-work on this device and for "keep whole project sessions on this device". When a session goes online by default while an equally capable local model is ready, the Model button says so.

- The Auto modes now judge models by how they actually run here, not by
  where their weights sit: a mixture-of-experts model running GPU + RAM
  counts as running well (it no longer loses to a small model just for
  using main memory), and a dense model that does not fit the graphics
  card is honestly "too large" instead of "runs slower".
- "Prefer fastest" ranks by each model's measured speed on this computer
  once you have used it; a model that is slow to load here must be
  clearly better before it replaces the one already loaded; and a
  question bigger than a model's reading room goes to a model that can
  hold it.
- Settings > Routing shows your models as routing sees them - how each
  runs, measured speed, load time, reading room, capability scores and
  project readiness - next to the recent decisions. Nothing new to set.

### Memory
- Memory is now two things, not four: each AI has ONE memory (entries you've
  given it, and moments it's learned with you), and what every AI knows
  about you lives in ONE place - Your Memory. Project sessions' notes are
  now called project notes, so "memory" always means those two.
- Your AIs now keep a short summary of who you are - a few sentences
  written on your device from your remembered facts and notes, shown at
  the top of Your Memory ("How your AIs see you") and given to every AI
  alongside the facts. It rewrites itself in the background as your memory
  changes - including updating things that have since happened - so if it
  is ever wrong, fix or forget the fact it came from and it follows.
  Duplicate learned facts tidy themselves up along the way. Nothing
  leaves your device, and pausing memory pauses this too.

### Reading room follows the turn
- A long attachment or a long conversation no longer fails with a raw
  "exceeds the available context" error. Before sending, the app checks
  whether the running model's context can hold the turn; if this
  computer can afford more, it reloads the same model with more room
  first - you see the usual loading hint - and then sends. If the
  server still finds the turn too big, the app grows to the exact count
  and sends again by itself.
- When nothing this machine can afford would hold it, the app says so in
  plain words - how much room the turn needs, how much the model is
  running with - and offers what the AI's mode allows: an
  online-and-offline AI can send it to an online model that holds it (an
  explicit choice for that turn, since an attachment would leave your
  device); an offline-only AI is never sent online quietly - the offer is
  to switch it to Auto - Online and Offline first; a pinned model gets
  "let Auto choose here". Health questions stay on your device whatever
  the size, and the online offer only appears when your plan includes
  online models.
- In Auto - Online and Offline, a turn too long for every model on your
  device routes to an online model that can hold it by itself (long
  conversations; attachments only with consent), and the reply says why.
- Models can now run with up to 131,072 tokens of context where the
  hardware carries it (the ceiling used to be 32,768 for every model, no
  matter the card or the model's own limit) - always sized against your
  graphics memory and clamped to what the model was trained for.
- The context cost of hybrid models (the Qwen 3.5 family, Nemotron 3.5,
  Granite 4, LFM2.5) is now counted from the layers that actually keep
  attention, not every layer - they were being charged several times
  their real cost, which kept them at smaller contexts than the card
  could carry and under-graded their fit.
- The attachment meter counts with the running model's own tokenizer and shows its real context, with the room the app can make for a longer turn - chips go amber when a reload will hold them and red only when nothing on this machine can.

### The app fails politely
- The first-run recommendation now sizes to your whole machine, not just
  the graphics card: a model that fits the card but not the system's
  memory is no longer offered. A big card in a machine with little memory
  used to be handed a model it could not load - and the app went down
  silently, every time.
- A model load that cannot succeed is refused with a plain sentence
  ("Not enough free memory to load this model safely right now - 1.4 GB
  free, about 2.7 GB needed. Close some other apps and try again, or pick
  a smaller model.") instead of the app vanishing.
- If the app ever stops mid-load, it will not automatically retry the
  same model on the next start - the crash loop is gone. Picking the
  model yourself still gets a fresh attempt.
- Mixture-of-experts models are labeled as such in the catalog (Ornith
  1.5's 35B is now "35B-A3B (MoE)"), which is also what unlocks their
  GPU + RAM split on cards they do not fit.

### Conversations

- The conversations list is ordered by last activity: continue a chat or
  a project from days ago and it comes back to the top, with the time of
  its last turn on the row (hover shows when it started). With more than
  one AI, a row of their avatars at the top filters the list to one AI.

### Projects
- The Projects tab (project notes shared by all your AIs) loads in a
  fraction of the time on installs with a long history: every AI's
  records are read at once instead of one after another, the last list
  shows instantly when you come back, and the "warming up" message only
  appears while records are genuinely still warming after launch.
- Project memory is now called project notes everywhere, and the Projects
  tab explains what projects are when you have none yet.
- A read of your records that stalls now gives up after a minute with a
  clear error instead of hanging the page.

### Code in replies

- Short code blocks (up to 14 lines) now stay right in the reply where they
  were written. Longer code is lifted into the code panel, and its first
  lines show in place, fading into a "Show all N lines" chip - so a reply
  can never read as if its code went missing
- The app's helper programs start with their console windows created
  hidden: no flash, and no Windows Terminal error dialog on machines where
  Windows Terminal is the default terminal. Helper programs left behind by
  an earlier launch that did not finish are cleaned up more thoroughly at
  the next start.

### Your records
- A conversation exists in your records from the moment you send your
  first message: it appears in the conversations list while the reply is
  still being written, and your words are kept even if the reply is
  stopped, fails, or the app closes mid-answer. (Nothing used to be
  written until the reply finished.)
- Stopping a reply keeps what was written so far on screen, marked
  "Stopped here", and records it as far as it got, flagged as stopped -
  so your records show the conversation as it actually happened.
- History stays reachable even when older storage generations are switched
  off - reading your records no longer depends on every generation being
  awake at once.

- Replies from online models now record the provider's own fingerprint
  for the backend that answered (when the provider sends one), alongside
  the model name: the provider's claim, inside your encrypted record.
  Offline replies already carry the hash of the exact model file.

### Other
- Loading a model now shows its proper catalog name ("Qwen 3.6 35B-A3B
  (MoE)", not a cleaned-up file name), and when a load takes more than a
  few seconds, short lines explain what the wait is - the model moving
  into memory, once - and that it stays loaded for the questions after.
  Fast loads show none of it.

- "Your AIs can now go online" shows once, the moment a plan activates.
- Removed the disabled records-tidying command.
- The Models page's picks for this computer rank by where a model would
  run first - on the card, then split with main memory, then processor -
  the same order the first-run recommendation uses, so both screens name
  the same model for the same machine.
- The offline models page carries the same header controls as every
  other page, Conversations included.

### Elsewhere
- Every model, offline and online, now has its own page at
  yourownai.net/models - with a check of what runs on your machine, the
  same sizing the app uses.

## [0.5.1] - 2026-08-21

### Quieter at launch

- Signed out, the app no longer contacts Flowsta's relay at launch. The
  online model catalog loads when you open Online Models or a model
  picker, or when you sign in - online is a choice you make, and the
  network traffic now says the same.

### Signing exports

- Signing an exported conversation with your Flowsta identity now
  handles the app's link to your Vault right in the flow: if the Vault
  doesn't recognize the app yet, it asks you to approve the link, then
  signs. Previously an unlinked install was refused with the Vault's
  policy message and the receipt saved unsigned.

### Fixes

- A model load can no longer leave the app showing "loading" forever -
  a load that doesn't finish reports itself and lets you carry on.

## [0.5.0] - 2026-08-20

### Your health questions, your choice of model

- Health questions are answered on your device by default, and you now
  choose which of your downloaded models answers them - Settings >
  Routing. Downloading a medical specialist offers the switch once.
- If you prefer an online model for health questions, that is your
  choice too - and the app asks before each question leaves your
  device, every time, unless you tell it to stop asking. Health images
  never go online.

### Agent permissions: one simple choice

- **Your AI can run ordinary project work without asking.** One
  three-level choice: Ask every time, Auto - ordinary work inside the
  project folder runs unasked while risky, irreversible, or
  outside-the-folder actions still ask - or Approve everything.
  Settings > Agent sets the default; the project chip in the header
  switches one project live, and a project's own choice sticks.
- Whatever you choose, every action goes in your records: automatic
  allows show as receipts on the rail, and each turn keeps a ledger of
  everything the agent was allowed to do. Permission answers record
  the decision, its scope, and how it was answered.
- Needs the Your Own AI Build update offered in the folder menu and in
  Settings > Components.

### Projects: the working session, reworked front to back

Working in a project with your AI got the deepest pass of this
release. The goal, in one line: you always know what your AI is doing,
and nothing it does makes you wait.

- **The rail always shows life.** The activity indicator sits at the
  true tip of the turn, so a long think after a spoken paragraph never
  looks dead. A conclusion your AI speaks mid-turn is never buried
  behind silent steps - it becomes the answer. A finished turn's
  summary says when its background task is still running, and the rail
  shows what that task is doing while the agent waits on it.
- **Nothing holds up your turn.** Remembering something for a project
  happens in the background, project memory opens instantly with
  cached and indexed reads, and reading the project's own memory no
  longer asks permission.
- **The project menu is rebuilt**: the project's name and live status
  in plain words, permissions as one control with its meaning always
  visible, and memory and close as clear tiles. A stopped project can
  be reopened right there.
- **Web, on your terms.** The Build agent's web search works through
  the AI's own routing - an offline-only AI stays off the web in
  project sessions, and says so. A silent online call is named for
  what it is: waiting on the provider.
- Opening a project conversation lands on its last message, and its
  history is whole - a conclusion spoken before silent steps shows as
  the answer even for conversations recorded before this release.

### Your records, fast and honest

- **The conversations drawer opens instantly.** Every AI's conversation
  list is kept as an encrypted local copy that shows immediately - even
  right after launch - while live reads refresh it in the background. A
  slow read never blanks what you can see, and deleting a conversation
  disappears from the list at once.
- An AI now always finds and keeps its existing records after launch -
  previously a slow start could leave an AI beginning a fresh set,
  with its earlier conversations present but out of view.
- Opening a conversation lands on its last message, names the
  conversation it is opening, and if your records don't answer in
  time it says so - nothing is lost - instead of showing an empty
  chat.
- Every memory surface says your records are warming up after launch
  instead of claiming "nothing yet" about records that exist.
- Loading spinners no longer slow the app down on machines where
  animations are expensive - pages stay responsive through the whole
  warmup.

### Models

- **The offline models page opens with "Best for this computer"** - one
  pick per activity (coding with you, everyday chat, seeing images,
  health questions), each chosen from what your machine can actually
  run, with download in place. The health pick can become your
  health-questions model in one tap. Downloaded models are now a
  compact list with your disk total, collapsible out of the way.
- **Every model card says who made the weights** - the maker, who
  packaged the files, and for community builds, what they are based on
  and how they differ.
- Recorded conversations now carry a fingerprint of the exact model
  file that answered, alongside its name - proof of which weights
  produced a response, sealed in your records.
- New in the offline catalog: Ornith 1.5 (the 35B now sees images),
  the Qwen 3.8 Distilled family (2B, 4B, and 9B - strong small models
  for modest hardware), Qwythos v2 (the repetition fixes), and Qwen
  3.8 Uncensored (27B, with the vision add-on).
- New online: GLM-5.3.
- Fixed: the Qwen 3.8 27B download had stopped working after its
  publisher renamed the file.
- Online chat turns send a per-conversation cache key, so consecutive
  turns reuse the provider's cache and bill at the cached-input rate;
  Online Models shows that rate.

### App updates

- The app can now tell you when a new version is out - a quiet note on
  the home screen linking to the download page, once per release.
  Nothing about you or your machine is sent; it reads one public file.
  Turn it off in Settings > Help & diagnostics.

### Fixes

- The hero "Continue" line shows its state the moment you click - a
  spinner while it opens, and the records-warming message when your
  records are still loading after launch.
- Long command lines in a turn's summary stay inside the window.
- The working rail's thinking indicator no longer occasionally doubles.
- Embedding inputs are capped to the model's context so one long query
  cannot fail a whole recall batch.

## [0.4.1] - 2026-08-16

### Offline models load on every Windows install

- **Fixed: on Windows installs outside the C: drive, the bundled engine
  could not start, so offline models never loaded** - the app reached
  "Starting server with model" and went silent. The engine was located
  through a path resolver that fails on custom-drive installs; it is now
  found relative to the app itself, the same way the record-keeping
  engines already were. Thanks to the tester whose patient diagnostic
  reports over three days led us to it.
- Graphics-card probes are bounded: a driver that hangs while enumerating
  devices no longer stalls every model load - after 15 seconds the load
  continues on the processor.
- Every way a model load can fail is now named in the diagnostic report,
  including a server that starts and produces no output at all.

### Deleting a conversation works for every AI

- **Fixed: deleting a conversation from an AI's records did nothing** for AIs
  created after the app first learned to delete - new AIs, AIs restored
  from your Vault, and every fresh install. The ability is now given to
  each AI's records the moment they are set up, existing ones are brought
  up to date on the next launch, and a delete that fails says so instead
  of closing quietly.

### Your records, warming up honestly

- After launch, the Memory page, the conversations drawer, and the home
  screen's "Continue last conversation" all show that your records are
  still warming up instead of an empty list, a false zero, or a dead
  click - and open the moment they are ready.
- Warmup copy says "ready", not "online" - these records never leave your
  device.

### Attachments

- Attached documents appear as file chips in your message, never as their
  extracted text - in regular chats and in project sessions alike.
- Attachments stay on your device unless "Send attachments to online
  models" is on. The setting now lives in Settings > Routing, because it
  decides both where the message goes and what may leave the machine, and
  the one setting governs regular chats and project sessions.

### Agent sessions, seen clearly

- Background work is named and visible: condensing working notes,
  extracting memories, and scripts still running in the background stay in
  the rail - even across a stopped and restarted turn.
- The rail names the real tool behind an agent's action ("Remember
  something for this project") instead of a generic "Use Tool".
- Agent sessions start faster: the online model list is cached for a
  minute instead of being fetched on every step.

### Routing and models

- A paused model is out of automatic routing's reach until you resume it,
  and the pause and resume tooltips say exactly that.
- Signed-in online accounts stay signed in as the service rotates their
  sign-in tokens.

### Under the hood

- Rust dependencies audited and updated with the record-keeping crates
  left byte-identical; the dependency lockfile is now committed so every
  build is reproducible.
- Front-end build tooling updated; the development build targets the same
  JavaScript level as the production build.

## [0.4.0] - 2026-08-15

### Qwen 3.8-27B, offline with vision

- Alibaba's newest open model joins the offline catalog - frontier-class
  coding, math, and reasoning with built-in thinking, Apache-2.0 licensed.
- Hybrid attention keeps long documents fast, with a 262K context window.
- An optional vision add-on lets it see images you attach.
- Best on 24GB-class graphics cards, or 32GB RAM on the processor.

### Your records now work on macOS

- Fixed: on Mac, the record-keeping engine crashed the first time it set
  up an AI's conversation records - every macOS install was silently
  affected, and new chats were never saved to your records.
- The cause was missing security entitlements on our bundled engine; it
  now ships with the same set Holochain's own desktop apps use, and
  records set up in seconds on first launch (confirmed on Apple Silicon).
- If your Mac ended up in "Running on CPU for stability" after
  force-quitting the broken app, one click on "Try GPU again" restores
  full speed - and macOS now tolerates force-quits without tripping that
  safety net.
- Huge thanks to the tester whose codesign-level bug report led us
  straight to it.

### Switch models where you are

- The model name on every AI card is now a switcher - click it to change
  that AI's model in place.
- A quiet chip beside the chat's Ask row shows the current model
  arrangement and switches it in one tap (Settings > Appearance can hide
  it for a bare chat).
- Same choices everywhere: automatic routing modes, your offline models
  with their fit dots, online models, or your own connected server.

### Honest on every graphics card

- When a graphics driver can't actually run models, the app now detects
  it, switches to your processor automatically, and says so plainly - with
  a driver-update hint when one would genuinely help. "Model too large" is
  no longer misused for driver failures.
- The optional NVIDIA CUDA engine is only offered on cards it supports.
- Model recommendations, fit badges, and context sizing all plan for the
  processor when the graphics card is out of play.
- Windows engine downloads are now code-signed.

### Under the hood

- Inference engine updated to llama.cpp b10435 - fixes Muse Glimmer
  occasionally losing a trailing tool call in agent work, plus sharper
  tool-call parsing for Qwen models.
- Automatic model picks no longer grab a bigger model while your current
  one is still loading - fit is judged as if the slot were free, so
  balanced routing stays balanced.
- On slow or busy machines, record setup now waits out the storage
  engine's warmup instead of showing "records couldn't be set up" too
  early.
- Diagnostic reports on macOS now include the actual crash cause from
  system crash records.

## [0.3.0] - 2026-08-13

### Muse Glimmer 30B, on your own hardware

- Meta's Muse Glimmer 30B joins the offline catalog - chat, image
  understanding, and real agent work in your project folders, running
  entirely on your device.
- Works on NVIDIA and AMD consumer graphics cards (optional
  high-performance NVIDIA engine available in-app).
- Inference engine updated to llama.cpp b10355.
- Reasoning strength adapts to the question - quick answers stay quick,
  hard problems get deep thinking.

### Bring your history

- Import your conversations from ChatGPT, Claude, and Perplexity exports.
- Import coding sessions from Claude Code, OpenCode, Codex, Cursor, and
  Aider - most auto-detected with one click.
- Adopted conversations keep their original dates, and your AIs remember
  what's in them.
- Your Memory page: filter learned facts by import, forget any batch in
  one click.

### Online models

- Grok 4.6 and Grok 4.6 (Web) available the day they launched - with
  support for the restored reasoning dial.
- DeepSeek V4 now serves its full 1M context; catalog shows every model's
  real context and release date.
- Online chats stream noticeably faster - connection reuse, lighter checks
  before the first token.
- Model cards show which shelf each model belongs to (chat, web search,
  coding) and sort correctly by newest.

### Smarter on your hardware

- Context size is now chosen from your graphics card and memory together -
  agent sessions get the room they need (fixes project work dying at 8K
  context on 32GB machines).
- Downloaded model cards show how each model fits this machine: full
  speed, runs slower, or too large - plus trained context vs what it runs
  at here.
- Auto model picks are fit-aware end to end: a model that struggles on
  your hardware hands off to one that runs at full speed, except while a
  project is open so your session's model stays warm.

### Diagnostics and reliability

- One-click diagnostic report (Settings > Help & diagnostics) - system,
  models, routing decisions, crash records; redacted, saved locally, never
  uploaded; also copyable straight to the clipboard.
- If the app ever gets stuck starting, the loading screen offers to save
  the same report - no working app required.
- The report lists every model file's health, so a damaged download can't
  hide.
- One app instance, enforced - a second launch or a leftover process can
  no longer break startup.
- Faster startup on machines where the record-keeping engine takes time
  to warm up.
- The NVIDIA engine and the Your Own AI Build agent both gained proper
  update flows for future releases.
- Copy buttons work reliably on Windows everywhere in the app.
- Windows installer is code-signed; an MSI package is now published
  alongside it.

## [0.2.0] - 2026-08-06

### Changed
- **Projects are dramatically faster.** Model metadata is now cached
  instead of re-read on every step - agent steps that took close to a
  minute on modest hardware now land in seconds, and opening a project
  is near-instant.
- **GPT-5.6 Sol drives project work by default**, and your own model
  picks from Settings now apply to project sessions too (they were
  silently ignored before).
- **Model choices respect your hardware honestly.** Recommendations
  count the memory that's actually free, integrated graphics is sized
  as what it is (with a truthful "Integrated graphics" card badge), and
  automatic picks prefer a model that runs comfortably over a smarter
  one that barely loads.
- **Privacy-first routing means what it says.** With the online lean
  set to privacy-first, hard questions stay on your device and only
  genuine live-web needs go online.
- Gemma 4 downloads use Google's official builds carrying their July
  refresh (better tool use, wider vision); the larger variants now
  show their true 256K context. Existing Gemma downloads keep working -
  re-download to get the refresh.
- The app opens at a roomier default size, the Your AIs cards say
  "Edit" where before there was only an icon, and every
  project-readiness notice says exactly what to look for offline: a
  model marked "Agentic" on the Offline Models page.

### Added
- **See your plan usage in Settings** - spend against your monthly
  allowance, with an optional live ticker in the header (off by
  default).
- **System Information with "Copy for support"** - everything that
  determines model fit, copyable in one click, with nothing personal
  included.
- **Project sessions warn before they can't work**: opening a folder
  with an offline-only AI now tells you up front if no downloaded
  model can drive project work, or if the capable one won't fit
  comfortably on your hardware.
- **Only one copy of the app runs at a time** - launching it again
  focuses the open window instead of starting a second instance that
  would fight the first over your models and graphics memory.
- Older log files are kept when the log rotates, so a problem's
  history survives an app restart.

### Fixed
- **Editing your selected AI now applies immediately.** Before, an
  open chat kept the AI's previous settings until you switched away -
  including its online/offline mode, which could route a message
  online after you had chosen Offline Only. Fixed, and this class of
  routing promise is now verified automatically across nearly two
  thousand setting combinations before each release.
- A model that crashed the app while loading is never automatically
  loaded again on the next start - no more crash loops that required
  deleting files by hand.
- A model too slow to load says so clearly and isn't retried that
  session, instead of failing with a raw error code.
- Models whose chat format can't support project work are no longer
  offered for it, whatever their name suggests.
- The "permission needed" button scrolls to the permission card
  instead of past it.

## [0.1.1] - 2026-08-04

### Fixed
- **Offline models load for every Windows user name.** On Windows accounts
  whose user folder contains non-ASCII characters (u with umlaut, Turkish
  letters, and friends), every offline model failed to load with a
  misleading "too large for your computer's memory" message. Model files
  are now opened in a way that works for every profile path, and a file
  that genuinely cannot be opened says so instead of blaming memory.
- **Restoring your Vault key works reliably on Windows.** Adopting the key
  from your Flowsta Vault could leave the app without a working
  conversation engine until reinstall. The restore now stops everything
  cleanly first, verifies the old state is fully released before switching
  keys, and aborts safely - changing nothing - when it cannot.
- Factory reset and app exit fully stop the background conversation engine
  on Windows.

### Added
- **Backups tell you when they are waiting.** If a backup attempt is held
  or fails - for example your Vault was just restored and wants its export
  imported first - the Flowsta Account section says so in plain words,
  with what to do next. Silence no longer looks like success.
- **Key recovery reads as one story.** When your Vault holds a different
  conversation key than this device, Backups & recovery walks you through
  it as two labeled steps: restore the key, then restore the
  conversations.

## [0.1.0] - 2026-07-29

The first stable release of Your Own AI - private AI on your machine.
No one in control but you.

- **Yours, offline.** Author AIs with their own personalities, portraits,
  memories, and knowledge. Chat with open models from the built-in
  catalog, GPU accelerated, with no account and nothing leaving your
  device.
- **Documents, with proof.** Attach files - scanned paper included, read
  on-device - and check any answer claim by claim against the exact
  wording in the source.
- **Projects.** Open a folder and your AI reads files, proposes edits,
  and runs commands, asking permission for every action - powered by the
  free Your Own AI Build add-on.
- **Memory you can read.** A shared profile, per-AI memories, and project
  memory - every entry visible, editable, and deletable.
- **Records you can prove.** Conversations are written into
  tamper-evident, signed records on your device. Export any
  conversation, and optionally sign it with your Flowsta identity.
- **Backed up, recoverable.** With Flowsta Vault connected, every
  conversation, AI, and memory backs up automatically and restores on a
  new device.
- **Share your AIs.** Export any AI as a signed pack others can import
  and verify - eight free characters are on yourownai.net.
- **Online when you want it.** Optional paid plans add frontier models
  and web search through a relay that strips your identity and never
  stores your messages. Everything local stays free, always.
- **An engine for other apps.** A local OpenAI-compatible endpoint
  serves your AIs - personality, memory, and records included - to
  editors, agent frameworks, and scripts, on this computer or your
  network.
- Installers are code signed on Windows and signed and notarized on
  macOS.

## [0.1.0-rc.2] - 2026-07-29

### Fixed
- The projects dropdown in the header no longer gets painted over by
  the AI selector row on the chat page - header menus now always stack
  above page content.

## [0.1.0-rc.1] - 2026-07-29

### Added
- The "How to connect" panel in External access links to step-by-step
  guides for Hermes Agent, OpenClaw, and other OpenAI-compatible apps
  at docs.yourownai.net.

### Changed
- Windows installers are now code signed - no more "Windows protected
  your PC" step during install. macOS builds remain signed and
  notarized as always.

## [0.1.0-beta.16] - 2026-07-29

### Added
- **Serve your AIs to your other devices.** A new toggle in Settings →
  External access opens the local endpoint to your network, minting an
  access key that other devices present as their API key - the same
  three-field setup every OpenAI-compatible app already uses, with Copy
  setup providing all three. Apps on this computer stay keyless, devices
  without the key are refused, and the key can be regenerated anytime.

## [0.1.0-beta.15] - 2026-07-28

### Added
- A "Reading ..." notice appears the moment any file is attached, so a
  large document never looks like nothing happened while it's read in.

### Changed
- The signed-export dialogs (Export AI and knowledge packs) now know
  whether Flowsta Vault is installed and unlocked, and guide you to the
  right next step for each case - including a direct link to get the
  Vault when it isn't installed. Exporting unsigned is always available.

## [0.1.0-beta.14] - 2026-07-27

### Fixed
- The monthly-allowance and sign-in notices now appear as proper cards in
  normal chat as well - the last remaining path that printed the raw error
  into the reply.

## [0.1.0-beta.13] - 2026-07-27

### Fixed
- Online-model billing and sign-in notices now appear as proper cards on
  every failure path during project work - one route was still printing
  the raw error into the chat.

## [0.1.0-beta.12] - 2026-07-27

### Changed
- The loaded-model indicator now sits first in the header, so the project
  chip and conversations stay together.

### Fixed
- Sign-in and plan notices now appear as proper cards during project work
  too - an online model needing attention mid-session no longer prints raw
  error text into the chat. Sending a new message clears the previous
  notice.

## [0.1.0-beta.11] - 2026-07-27

### Changed
- **A new app icon.** The mark in neutral chrome on a black tile, sized to
  be seen.
- **Conversation backups that grow with you.** With the newest Flowsta
  Vault, each conversation backs up as its own compressed object - there
  is no overall size budget, a single long session can never be too large
  to protect, and backup runs only re-send conversations with new
  messages. Older Vaults keep the single-snapshot backup they have today.
- The NVIDIA graphics-card speed tip now sits below the message field on
  the home page.
- Backups copy in Settings now describes the one recovery story: your
  key and conversations back up to your Flowsta Vault together.

## [0.1.0-beta.10] - 2026-07-27

### Added
- **Projects: agentic coding in chat.** Open a project folder and your AI
  works in it - reads files, makes edits, and runs commands, always with
  your permission. Every step is recorded in your private transcripts.
  Projects need Your Own AI Build, a free add-on downloaded with one
  click from inside the app (the project menu or the Components page).
- **See the work as it happens.** A working turn shows its whole story
  live: each step with a readable label and its real result, the model's
  thinking, file edits as colored diffs, and long-running commands
  streaming their output - full scrollback while they run, following the
  newest line until you scroll up. When the turn ends it folds into a
  single summary line you can reopen any time. A "Simple project view"
  setting trims the verbosity, never the liveness.
- **Project memory.** Each project keeps a memory all your AIs share:
  an AI can deliberately save a note while it works (you see exactly
  what it wants to save before allowing it), each session's takeaways
  are distilled when you finish, and you can edit every line yourself.
  Remember something "for this project" or "for this AI" - your choice.
- **Conversations pick up where they left off.** Reopening a project
  conversation restores what the AI knew, so it remembers the plans and
  commands from earlier instead of starting blank.
- **Cost-aware routing for projects.** Side tasks run on your device when
  your hardware is comfortably up to it, and a thrifty setting keeps
  whole project sessions on-device when they fit. The routing settings
  explain exactly what happens and when.

### Fixed
- A reply could occasionally go missing from your records when two
  saves landed at the same moment. Saving is now serialized per
  identity and retried, and long results are trimmed to fit instead of
  failing silently.

## [0.1.0-beta.9] - 2026-07-17

### Added
- **Live web-search progress.** While an online search model researches,
  the status line shows each step - "Searching the web (3)..." and the
  actual queries it ran - instead of a silent wait. Deep research turns
  used to look frozen for 30-60 seconds while working perfectly.

### Changed
- **Online questions start much sooner.** The pre-checks that decide how a
  message is handled now run alongside routing instead of before it, and
  they no longer wait behind a model that's still loading. The worst case
  - an online question right after launch - used to stall for many
  seconds before anything happened.
- **Honest status lines.** The model-loading indicator only appears when
  your question is actually waiting on a model load, and it names the
  model. A background warm-up no longer badges an unrelated reply with a
  bare loading icon.
- **Thinking text reads properly.** The thinking box and the Thoughts
  view both render formatting the same way - no more doubled paragraph
  gaps in one and raw markdown in the other - and headings inside
  reasoning render small instead of shouting.
- Questions about upcoming fixtures and betting odds now count as
  needing current information and go to the web.

### Fixed
- A search model's reasoning could print a stray sentence (or more) into
  the reply when it kept thinking between searches. Reasoning now always
  lands in the thinking box. (Server-side - this also repairs older
  betas.)

## [0.1.0-beta.8] - 2026-07-17

### Fixed
- **Everyday questions no longer route to your medical model.** Health
  detection is now measured and precise: a question must genuinely read as
  being about your health, not merely resemble one. Previously questions
  like "whats the latest news" could be kept off the web and answered by
  the medical model.
- **A medical model can't take over general chat.** Specialist models only
  answer the questions they're for; once a health turn is done, the next
  ordinary question switches back to your general model instead of
  sticking with the specialist.
- **MedGemma's reasoning now shows in the thinking box** instead of
  printing inside the reply (which used to start with a stray "thought").
- **Hard questions sent online answer again.** The online service rejected
  a request setting used with the newest models; fixed on our servers, so
  this also repairs older betas.

### Added
- **Colors and gradients in the thumbnail gallery** - ten solid colors and
  ten soft radial gradients for a clean, professional look. The gallery is
  now grouped as Colors, Gradients, People, and Characters (the default
  AIs' portraits live in People and Characters now, not a separate group).

## [0.1.0-beta.7] - 2026-07-17

### Added
- **Kimi models join the online catalog.** Kimi K3 - Moonshot AI's new
  flagship with deep reasoning, a huge context window, and image
  understanding - plus the remarkably low-cost Kimi K2.6 and the dedicated
  Kimi K2.7 Code.
- **A better Online Models page.** Now matches the Offline Models layout:
  filter tabs (All, Chat, Web search, Coding) with model counts over a
  single grid, and a sort control - newest, name, or price. Each card shows
  which categories the model belongs to, and an "Auto pick" badge marks the
  models automatic routing already uses on your behalf.

### Changed
- A model can now appear in more than one category - Kimi K3 and GPT-5.6
  Sol show under both Chat and Coding, where they belong.

## [0.1.0-beta.6] - 2026-07-17

### Added
- **Medical models.** MedGemma 4B and 27B - Google's health-tuned models,
  both able to read images - join the model catalog, with a GPT-OSS 120B
  variant for high-memory machines. Medical models download after a short
  agree-to-the-publisher's-terms step, shown right in the download flow.
- **Health questions stay on your device.** In the online-and-offline Auto
  mode, a question about your health never auto-routes to an online model:
  it stays local and prefers your medical model if you have one - photos of
  skin, X-rays, and scans included. The "Model" note under the reply tells
  you when this happened.
- **"Remember this" everywhere.** Save any reply, highlighted passage, or
  transcript entry into memory: a Remember button under each reply, a
  floating chip when you highlight text in a reply, and a button under each
  entry on the memory page.
- **Remember is reversible, and you choose where saves go.** Every Remember
  button is a toggle - click "Remembered" to forget it again, even after a
  restart. A new Settings - Memory section picks the destination for each
  kind of save (kept to that AI only, or shared notes every AI can draw on)
  and gathers the automatic-learning switch.

### Fixed
- Downloading a vision model now says what's happening when it fetches the
  second file (the small projector that lets the model read images) instead
  of showing a second unlabeled progress bar.
- Image questions no longer stall when the preferred vision model is too
  large for your hardware - they fall back to an image-capable model that
  fits.
- Dropdowns opened near the bottom of the edit-AI dialog now scroll into
  view instead of being clipped.

## [0.1.0-beta.5] - 2026-07-16

### Changed
- **Your first model no longer gets pinned to your AIs.** On a fresh install,
  AIs are set to "Auto - Offline Only", so they automatically use the best
  model you have as your collection grows, instead of staying on the starter
  model forever.
- **Smoother, lighter streaming.** Replies render with far less overhead
  while generating - most noticeable on long reports with fast graphics
  cards - and switching models mid-conversation starts the reply about half
  a second sooner.

## [0.1.0-beta.4] - 2026-07-16

### Added
- **Give your AIs documents.** Editing an AI now has a Knowledge tab: add
  files (PDF, Word, Excel, text, code) and the AI reads and remembers them,
  drawing on the relevant parts in any conversation. Original files can be
  moved or deleted afterward - the AI keeps its own copy. A large document
  costs nothing extra per message: only the pieces that matter are used.
  Documents also show on the AI's memory page, where they can be added too.
- **A cleaner edit-AI dialog.** Editing an AI now uses a wider layout with
  sections - Basics, Behaviour, Details, Knowledge, Appearance - instead of
  one long scroll. Creating a new AI stays quick and simple.
- **The NVIDIA engine is offered on the home screen.** If your graphics card
  qualifies and the high-performance engine isn't installed, a dismissible
  tip lets you download it right there - no trip to Settings.

### Changed
- **Attached files now show as chips in the message box** - name, size, and
  a remove button - instead of a list below it. Images appear as thumbnails
  in the same row. A context-usage note appears only when attachments start
  to crowd the model's memory.

## [0.1.0-beta.3] - 2026-07-16

The inference engine now runs on clean Windows and macOS machines - beta 2's
"model couldn't be loaded" errors were the engine failing to start at all.

### Fixed
- **Windows: the engine no longer needs Microsoft's C++ runtime installed.**
  On machines with an older Visual C++ redistributable the engine crashed
  instantly and the app misread it as "model too large". The runtime is now
  built into the engine itself, so nothing on the machine matters.
- **macOS: the engine no longer depends on Homebrew.** It was linked against
  a Homebrew OpenSSL library that only exists on build machines, so it died
  on launch on real Macs. It now has no such dependency.
- **Honest errors when a model fails to load.** "Too large for your graphics
  card" is now only shown when the engine actually ran out of memory; an
  engine crash says so instead of sending you hunting for smaller models.

### Changed
- **New app icon.** The mark now sits on a dark gradient tile so it reads
  clearly on any desktop, light or dark, at every size.

## [0.1.0-beta.2] - 2026-07-16

Fixes from the first day of beta testing on Windows and low-memory Macs.

### Fixed
- **Windows: the app can actually run models now.** The bundled inference
  engine linked two OpenSSL libraries that aren't present on a normal
  Windows machine, so it crashed on launch with missing-DLL errors. The
  engine is rebuilt with no OpenSSL dependency, and the build now fails if
  that dependency ever sneaks back in.
- **Windows: no more stray terminal windows.** The background services the
  app starts (key store, conversation store, inference engine) had visible
  console windows; they are now hidden automatically.
- **Windows: closing and reopening the app works.** Background services are
  now tied to the app's lifetime, so quitting can't leave orphans behind
  that block the next launch from recording conversations.
- **Small-memory machines get honest model recommendations.** On an 8GB
  MacBook Air the welcome screen recommended a 16.8GB model and the models
  page showed nothing as runnable. Apple Silicon's shared memory is now
  detected as the GPU budget, the memory reserve scales with machine size,
  and the first-run recommendation uses the same fit math as the models
  page.

### Changed
- Beta builds are not Windows code-signed (SmartScreen will ask once);
  release candidates and stable releases are signed as usual.

## [0.1.0-beta.1] - 2026-07-15

First public beta.

### Highlights
- Chat with AI models running entirely on your machine - a curated catalog of
  open models sized from 2B to 32B, with hardware-aware recommendations so you
  only see what your computer can actually run.
- Auto model routing: let the app pick the best model per question, offline
  only or a mix of offline and online, with your choices for which online
  model handles current-info and hard questions.
- Online models (with a plan): the GPT-5.6 family, Grok 4.5, Grok Build, and
  Sonar - including live web search with cited sources.
- Every reply shows which model answered and why (the Model button under each
  answer), and can be redone on your device or online with one click.
- Persistent memory: your AIs remember facts you teach them and past
  conversations, stored encrypted on your machine.
- Attachments and vision: images, documents, and OCR for scanned PDFs.
- Conversation history is kept as an append-only encrypted transcript, with
  signed Markdown export.
- GPU acceleration out of the box (Vulkan on Linux and Windows, Metal on
  macOS), an optional high-performance CUDA engine download for NVIDIA cards,
  and the option to connect your own inference server.
- Back up and restore everything through your Flowsta Vault.

### Known beta limitations
- No auto-update yet: install new betas manually.
- Windows CUDA engine is built but not yet verified on hardware.
