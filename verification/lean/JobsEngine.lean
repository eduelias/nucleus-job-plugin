/-!
# Model of the job engine's de-duplication (`src/jobs/engine.rs`)

`Engine::process_job` claims a job id in `seen` before doing anything, so the
repeated `notify-next` / `start-next` deliveries AWS sends for one job are
skipped. This file models that, over every possible delivery order, and proves:

* `ran_nodup`: a handler never runs twice for the same job id.
* `retry_after_failed_publish`: when the first status publish fails (the
  handler has not run yet), the claim is released and a later delivery runs it.
* `stepOld_strands_job`: the earlier code kept the claim on that failure, so the
  job was skipped for the rest of the session.

The model is hand-written, not extracted: keep `step` in sync with `process_job`.
-/

namespace JobsEngine

abbrev JobId := Nat

structure State where
  /-- Claimed job ids (`Engine::seen`). -/
  seen : List JobId := []
  /-- Handler executions, newest first. -/
  ran : List JobId := []

/-- One delivery of a job to `process_job`. -/
structure Delivery where
  job : JobId
  /-- `Action::from_document` succeeds. -/
  validDoc : Bool
  /-- The first status publish succeeds: `IN_PROGRESS`, or `FAILED` for a bad
  document. Later publishes don't change the state, so they are not modelled. -/
  publishOk : Bool

/-- `process_job` as it is now. -/
def step (s : State) (d : Delivery) : State :=
  if d.job ∈ s.seen then s                        -- already claimed: skip
  else if !d.publishOk then s                     -- publish failed: release claim
  else if !d.validDoc then { s with seen := d.job :: s.seen }   -- reported FAILED
  else { seen := d.job :: s.seen, ran := d.job :: s.ran }        -- handler ran

/-- `process_job` before the fix: the claim survives a failed publish. -/
def stepOld (s : State) (d : Delivery) : State :=
  if d.job ∈ s.seen then s
  else if !d.publishOk then { s with seen := d.job :: s.seen }
  else if !d.validDoc then { s with seen := d.job :: s.seen }
  else { seen := d.job :: s.seen, ran := d.job :: s.ran }

def run (f : State → Delivery → State) (s : State) (ds : List Delivery) : State :=
  ds.foldl f s

/-- Every job that ran is claimed, and none ran twice. -/
def Inv (s : State) : Prop := (∀ j ∈ s.ran, j ∈ s.seen) ∧ s.ran.Nodup

theorem step_inv (s : State) (d : Delivery) (h : Inv s) : Inv (step s d) := by
  obtain ⟨hsub, hnd⟩ := h
  by_cases hs : d.job ∈ s.seen
  · have e : step s d = s := by simp [step, hs]
    rw [e]; exact ⟨hsub, hnd⟩
  by_cases hp : d.publishOk = false
  · have e : step s d = s := by simp [step, hs, hp]
    rw [e]; exact ⟨hsub, hnd⟩
  by_cases hv : d.validDoc
  · have e : step s d = { seen := d.job :: s.seen, ran := d.job :: s.ran } := by
      simp [step, hs, hp, hv]
    rw [e]
    refine ⟨fun j hj => ?_, List.nodup_cons.mpr ⟨fun hr => hs (hsub _ hr), hnd⟩⟩
    rcases List.mem_cons.mp hj with rfl | hj
    · exact List.mem_cons_self
    · exact List.mem_cons_of_mem _ (hsub j hj)
  · have e : step s d = { s with seen := d.job :: s.seen } := by simp [step, hs, hp, hv]
    rw [e]
    exact ⟨fun j hj => List.mem_cons_of_mem _ (hsub j hj), hnd⟩

theorem run_inv (ds : List Delivery) : ∀ s, Inv s → Inv (run step s ds) := by
  induction ds with
  | nil => intro s h; exact h
  | cons d ds ih => intro s h; exact ih _ (step_inv s d h)

/-- From a fresh engine, no delivery sequence runs a handler twice for one job. -/
theorem ran_nodup (ds : List Delivery) : (run step {} ds).ran.Nodup :=
  (run_inv ds {} ⟨by simp, by simp⟩).2

/-- A failed first publish leaves the job unclaimed and the state unchanged... -/
theorem failed_publish_keeps_state (s : State) (d : Delivery) (hp : d.publishOk = false) :
    step s d = s := by
  unfold step; by_cases hs : d.job ∈ s.seen <;> simp [hs, hp]

/-- ...so the next good delivery of that job runs its handler. -/
theorem retry_after_failed_publish (s : State) (j : JobId) (hj : j ∉ s.seen) :
    (run step s [⟨j, true, false⟩, ⟨j, true, true⟩]).ran = j :: s.ran := by
  simp [run, step, hj]

/-- The old code never runs a job whose first publish failed. -/
theorem stepOld_strands_job :
    (run stepOld {} [⟨1, true, false⟩, ⟨1, true, true⟩]).ran = [] := by
  decide

end JobsEngine
