/-!
# Model of the job engine's de-duplication (`src/jobs/engine.rs`)

`Engine::process_job` claims a job id in `seen` before doing anything, so the
repeated `notify-next` / `start-next` deliveries AWS sends for one job are
skipped. This file models that over every possible delivery order, within one
run of the runner (a restart forgets `seen`, and AWS hands back any job still
`IN_PROGRESS`), and proves:

* `ran_nodup`: a handler never runs twice for the same job id.
* `retry_after_failed_publish`: when the first status publish fails (the
  handler has not run yet), the claim is released and a later delivery runs it.
* `resend_after_failed_final`: when the final status fails to publish, a later
  delivery re-sends it without running the handler again.
* `stepOld_strands_job`: the earlier code kept the claim on a failed first
  publish, so the job was skipped for the rest of the run.

Deliveries are arbitrary here; in the code, the engine re-requests `start-next`
after a transport error so that one arrives. The model is hand-written, not
extracted: keep `step` in sync with `process_job`.
-/

namespace JobsEngine

abbrev JobId := Nat

structure State where
  /-- Claimed job ids (`Engine::seen`). -/
  seen : List JobId := []
  /-- Handler executions, newest first. -/
  ran : List JobId := []
  /-- Jobs whose final status failed to publish (`Engine::unsent`). -/
  unsent : List JobId := []

/-- One delivery of a job to `process_job`. -/
structure Delivery where
  job : JobId
  /-- `Action::from_document` succeeds. -/
  validDoc : Bool
  /-- The first publish of this delivery succeeds: `IN_PROGRESS`, `FAILED` for a
  bad document, or the re-sent final status for a claimed job. -/
  publishOk : Bool
  /-- The final status publish after the handler runs succeeds. -/
  finalOk : Bool

/-- `process_job` as it is now. -/
def step (s : State) (d : Delivery) : State :=
  if d.job ∈ s.seen then
    -- Claimed: re-send a final status that failed to publish, otherwise skip.
    if d.job ∈ s.unsent ∧ d.publishOk = true then
      { s with unsent := s.unsent.erase d.job }
    else s
  else if d.publishOk = false then s                    -- publish failed: release claim
  else if d.validDoc = false then { s with seen := d.job :: s.seen }   -- reported FAILED
  else if d.finalOk = true then
    { s with seen := d.job :: s.seen, ran := d.job :: s.ran }          -- ran and reported
  else
    { seen := d.job :: s.seen, ran := d.job :: s.ran, unsent := d.job :: s.unsent }

/-- `process_job` before the fixes: the claim survives a failed publish. -/
def stepOld (s : State) (d : Delivery) : State :=
  if d.job ∈ s.seen then s
  else if d.publishOk = false || d.validDoc = false then { s with seen := d.job :: s.seen }
  else { s with seen := d.job :: s.seen, ran := d.job :: s.ran }

def run (f : State → Delivery → State) (s : State) (ds : List Delivery) : State :=
  ds.foldl f s

/-- Every job that ran is claimed, and none ran twice. -/
def Inv (s : State) : Prop := (∀ j ∈ s.ran, j ∈ s.seen) ∧ s.ran.Nodup

/-- A step that keeps `seen` and `ran` keeps the invariant. -/
theorem inv_of_same {s t : State} (hs : t.seen = s.seen) (hr : t.ran = s.ran) (h : Inv s) :
    Inv t := by
  rw [Inv, hs, hr]; exact h

theorem step_inv (s : State) (d : Delivery) (h : Inv s) : Inv (step s d) := by
  obtain ⟨hsub, hnd⟩ := h
  by_cases hs : d.job ∈ s.seen
  · by_cases hu : d.job ∈ s.unsent ∧ d.publishOk = true
    · exact inv_of_same (by simp [step, hs, hu]) (by simp [step, hs, hu]) ⟨hsub, hnd⟩
    · exact inv_of_same (by simp [step, hs, hu]) (by simp [step, hs, hu]) ⟨hsub, hnd⟩
  by_cases hp : d.publishOk = false
  · exact inv_of_same (by simp [step, hs, hp]) (by simp [step, hs, hp]) ⟨hsub, hnd⟩
  by_cases hv : d.validDoc = false
  · refine ⟨fun j hj => ?_, ?_⟩
    · have hr : (step s d).ran = s.ran := by simp [step, hs, hp, hv]
      have hse : (step s d).seen = d.job :: s.seen := by simp [step, hs, hp, hv]
      rw [hr] at hj; rw [hse]; exact List.mem_cons_of_mem _ (hsub j hj)
    · have hr : (step s d).ran = s.ran := by simp [step, hs, hp, hv]
      rw [hr]; exact hnd
  -- The handler runs: both `seen` and `ran` gain `d.job`, whatever `finalOk` is.
  have hr : (step s d).ran = d.job :: s.ran := by
    by_cases hf : d.finalOk = true <;> simp [step, hs, hp, hv, hf]
  have hse : (step s d).seen = d.job :: s.seen := by
    by_cases hf : d.finalOk = true <;> simp [step, hs, hp, hv, hf]
  refine ⟨fun j hj => ?_, ?_⟩
  · rw [hr] at hj; rw [hse]
    rcases List.mem_cons.mp hj with rfl | hj
    · exact List.mem_cons_self
    · exact List.mem_cons_of_mem _ (hsub j hj)
  · rw [hr]; exact List.nodup_cons.mpr ⟨fun h => hs (hsub _ h), hnd⟩

theorem run_inv (ds : List Delivery) : ∀ s, Inv s → Inv (run step s ds) := by
  induction ds with
  | nil => intro s h; exact h
  | cons d ds ih => intro s h; exact ih _ (step_inv s d h)

/-- From a fresh engine, no delivery sequence runs a handler twice for one job. -/
theorem ran_nodup (ds : List Delivery) : (run step {} ds).ran.Nodup :=
  (run_inv ds {} ⟨by simp, by simp⟩).2

/-- A failed first publish leaves the job unclaimed, so the next good delivery
runs its handler. -/
theorem retry_after_failed_publish (s : State) (j : JobId) (hj : j ∉ s.seen) :
    (run step s [⟨j, true, false, true⟩, ⟨j, true, true, true⟩]).ran = j :: s.ran := by
  simp [run, step, hj]

/-- A lost final status is re-sent on the next delivery, and the handler is not
run again. -/
theorem resend_after_failed_final (s : State) (j : JobId) (hj : j ∉ s.seen) :
    let t := run step s [⟨j, true, true, false⟩, ⟨j, true, true, true⟩]
    t.ran = j :: s.ran ∧ t.unsent = s.unsent := by
  simp [run, step, hj]

/-- The old code never runs a job whose first publish failed. -/
theorem stepOld_strands_job :
    (run stepOld {} [⟨1, true, false, true⟩, ⟨1, true, true, true⟩]).ran = [] := by
  decide

end JobsEngine
