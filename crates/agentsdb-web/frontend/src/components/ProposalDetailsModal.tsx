import type { ProposalRow } from '../types';

interface ProposalDetailsModalProps {
  proposal: ProposalRow | null;
  onClose: () => void;
}

export function ProposalDetailsModal({ proposal, onClose }: ProposalDetailsModalProps) {
  if (!proposal) return null;

  const createdDate = proposal.created_at_unix_ms
    ? new Date(proposal.created_at_unix_ms).toLocaleString()
    : 'Unknown';

  const decidedDate = proposal.decided_at_unix_ms
    ? new Date(proposal.decided_at_unix_ms).toLocaleString()
    : null;

  return (
    <dialog class="modal modal-open">
      <div class="modal-box max-w-3xl">
        <div class="flex justify-between items-start mb-4">
          <div>
            <h3 class="font-bold text-lg">
              Proposal <span class="mono">#{proposal.proposal_id}</span>
            </h3>
            <div class="text-sm text-base-content/70 mt-1">
              Context ID: <span class="mono">{proposal.context_id}</span>
            </div>
          </div>
          <button class="btn btn-sm btn-circle btn-ghost" onClick={onClose}>
            ✕
          </button>
        </div>

        <div class="space-y-4">
          <div>
            <div class="font-semibold text-sm mb-1">Status</div>
            <span
              class={`badge ${
                proposal.status === 'pending'
                  ? 'badge-warning'
                  : proposal.status === 'accepted'
                    ? 'badge-success'
                    : 'badge-error'
              }`}
            >
              {proposal.status}
            </span>
          </div>

          <div>
            <div class="font-semibold text-sm mb-1">Flow</div>
            <div class="mono text-sm bg-base-300 p-2 rounded">
              {proposal.from_path} → {proposal.to_path}
            </div>
          </div>

          {proposal.title && (
            <div>
              <div class="font-semibold text-sm mb-1">Title</div>
              <div class="text-sm">{proposal.title}</div>
            </div>
          )}

          {proposal.why && (
            <div>
              <div class="font-semibold text-sm mb-1">Why (Rationale)</div>
              <div class="text-sm bg-base-300 p-3 rounded whitespace-pre-wrap">{proposal.why}</div>
            </div>
          )}

          {proposal.what && (
            <div>
              <div class="font-semibold text-sm mb-1">What (Description)</div>
              <div class="text-sm bg-base-300 p-3 rounded whitespace-pre-wrap">{proposal.what}</div>
            </div>
          )}

          {proposal.where && (
            <div>
              <div class="font-semibold text-sm mb-1">Where (Location)</div>
              <div class="text-sm mono bg-base-300 p-2 rounded">{proposal.where}</div>
            </div>
          )}

          <div class="divider"></div>

          <div class="grid grid-cols-2 gap-4 text-sm">
            <div>
              <div class="font-semibold mb-1">Exists in Source</div>
              <div class={proposal.exists_in_source ? 'text-success' : 'text-error'}>
                {proposal.exists_in_source ? '✓ Yes' : '✗ No'}
              </div>
            </div>
            <div>
              <div class="font-semibold mb-1">Exists in Target</div>
              <div class={proposal.exists_in_target ? 'text-success' : 'text-error'}>
                {proposal.exists_in_target ? '✓ Yes' : '✗ No'}
              </div>
            </div>
            <div>
              <div class="font-semibold mb-1">Exists in Delta</div>
              <div class={proposal.exists_in_delta ? 'text-success' : 'text-error'}>
                {proposal.exists_in_delta ? '✓ Yes' : '✗ No'}
              </div>
            </div>
            <div>
              <div class="font-semibold mb-1">Exists in User</div>
              <div class={proposal.exists_in_user ? 'text-success' : 'text-error'}>
                {proposal.exists_in_user ? '✓ Yes' : '✗ No'}
              </div>
            </div>
          </div>

          <div class="divider"></div>

          <div>
            <div class="font-semibold text-sm mb-1">Created</div>
            <div class="text-sm mono">{createdDate}</div>
          </div>

          {decidedDate && (
            <>
              <div>
                <div class="font-semibold text-sm mb-1">Decided</div>
                <div class="text-sm mono">{decidedDate}</div>
              </div>
              {proposal.decided_by && (
                <div>
                  <div class="font-semibold text-sm mb-1">Decided By</div>
                  <div class="text-sm mono">{proposal.decided_by}</div>
                </div>
              )}
              {proposal.decision_reason && (
                <div>
                  <div class="font-semibold text-sm mb-1">Decision Reason</div>
                  <div class="text-sm bg-base-300 p-3 rounded whitespace-pre-wrap">
                    {proposal.decision_reason}
                  </div>
                </div>
              )}
            </>
          )}
        </div>

        <div class="modal-action">
          <button class="btn" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
      <form method="dialog" class="modal-backdrop" onClick={onClose}>
        <button>close</button>
      </form>
    </dialog>
  );
}
