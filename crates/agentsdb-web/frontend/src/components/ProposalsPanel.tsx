import { useState } from 'preact/hooks';
import type { ProposalRow } from '../types';

interface ProposalsPanelProps {
  proposals: ProposalRow[];
  loading?: boolean;
  onAccept: (ids: number[], skipExisting: boolean) => Promise<void>;
  onReject: (ids: number[], reason?: string) => Promise<void>;
  onViewDetails: (proposal: ProposalRow) => void;
  onRefresh: () => void;
}

export function ProposalsPanel({
  proposals,
  loading = false,
  onAccept,
  onReject,
  onViewDetails,
  onRefresh,
}: ProposalsPanelProps) {
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set());
  const [showAll, setShowAll] = useState(false);
  const [skipExisting, setSkipExisting] = useState(true);
  const [submitting, setSubmitting] = useState(false);

  const pendingProposals = proposals.filter((p) => p.status === 'pending');
  const displayedProposals = showAll ? proposals : pendingProposals;

  const toggleSelection = (id: number) => {
    const newSet = new Set(selectedIds);
    if (newSet.has(id)) {
      newSet.delete(id);
    } else {
      newSet.add(id);
    }
    setSelectedIds(newSet);
  };

  const selectAll = () => {
    setSelectedIds(new Set(displayedProposals.map((p) => p.proposal_id)));
  };

  const clearSelection = () => {
    setSelectedIds(new Set());
  };

  const handleAccept = async () => {
    if (selectedIds.size === 0) return;
    try {
      setSubmitting(true);
      await onAccept(Array.from(selectedIds), skipExisting);
      setSelectedIds(new Set());
    } finally {
      setSubmitting(false);
    }
  };

  const handleReject = async () => {
    if (selectedIds.size === 0) return;
    const reason = prompt('Reason for rejection (optional):');
    try {
      setSubmitting(true);
      await onReject(Array.from(selectedIds), reason || undefined);
      setSelectedIds(new Set());
    } finally {
      setSubmitting(false);
    }
  };

  if (proposals.length === 0) {
    return (
      <div class="card bg-base-200 shadow-xl">
        <div class="card-body">
          <h2 class="card-title">Proposals</h2>
          <div class="text-sm text-base-content/70">No proposals found.</div>
        </div>
      </div>
    );
  }

  return (
    <div class="card bg-base-200 shadow-xl">
      <div class="card-body">
        <div class="flex justify-between items-center mb-4">
          <div>
            <h2 class="card-title">Proposals</h2>
            <div class="text-sm text-base-content/70 mt-1">
              {pendingProposals.length} pending • {proposals.length} total
              {selectedIds.size > 0 && ` • ${selectedIds.size} selected`}
            </div>
          </div>
          <button
            class="btn btn-sm btn-ghost"
            onClick={onRefresh}
            disabled={loading || submitting}
            title="Refresh proposals"
          >
            <svg
              class="h-4 w-4"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
            >
              <path d="M21 12a9 9 0 1 1-2.64-6.36" />
              <path d="M21 3v6h-6" />
            </svg>
          </button>
        </div>

        <div class="flex flex-wrap gap-2 mb-4">
          <button class="btn btn-sm" onClick={selectAll} disabled={submitting}>
            Select All
          </button>
          <button class="btn btn-sm" onClick={clearSelection} disabled={submitting}>
            Clear
          </button>
          <div class="divider divider-horizontal"></div>
          <button
            class="btn btn-sm btn-success"
            onClick={handleAccept}
            disabled={selectedIds.size === 0 || submitting}
          >
            {submitting ? (
              <span class="loading loading-spinner loading-xs"></span>
            ) : (
              `Accept (${selectedIds.size})`
            )}
          </button>
          <button
            class="btn btn-sm btn-error"
            onClick={handleReject}
            disabled={selectedIds.size === 0 || submitting}
          >
            Reject ({selectedIds.size})
          </button>
          <div class="divider divider-horizontal"></div>
          <label class="label cursor-pointer gap-2">
            <span class="label-text text-xs">Skip existing</span>
            <input
              type="checkbox"
              class="checkbox checkbox-xs"
              checked={skipExisting}
              onChange={(e) => setSkipExisting((e.target as HTMLInputElement).checked)}
              disabled={submitting}
            />
          </label>
          <label class="label cursor-pointer gap-2">
            <span class="label-text text-xs">Show all</span>
            <input
              type="checkbox"
              class="checkbox checkbox-xs"
              checked={showAll}
              onChange={(e) => setShowAll((e.target as HTMLInputElement).checked)}
              disabled={submitting}
            />
          </label>
        </div>

        {loading ? (
          <div class="flex justify-center py-8">
            <span class="loading loading-spinner loading-lg"></span>
          </div>
        ) : (
          <div class="overflow-x-auto">
            <table class="table table-zebra table-sm">
              <thead>
                <tr>
                  <th>
                    <input
                      type="checkbox"
                      class="checkbox checkbox-sm"
                      checked={
                        displayedProposals.length > 0 &&
                        displayedProposals.every((p) => selectedIds.has(p.proposal_id))
                      }
                      onChange={(e) =>
                        (e.target as HTMLInputElement).checked ? selectAll() : clearSelection()
                      }
                      disabled={submitting}
                    />
                  </th>
                  <th>ID</th>
                  <th>Context</th>
                  <th>Flow</th>
                  <th>Status</th>
                  <th>Title</th>
                  <th>Actions</th>
                </tr>
              </thead>
              <tbody>
                {displayedProposals.map((proposal) => (
                  <tr key={proposal.proposal_id}>
                    <td>
                      <input
                        type="checkbox"
                        class="checkbox checkbox-sm"
                        checked={selectedIds.has(proposal.proposal_id)}
                        onChange={() => toggleSelection(proposal.proposal_id)}
                        disabled={submitting}
                      />
                    </td>
                    <td class="mono">{proposal.proposal_id}</td>
                    <td class="mono">{proposal.context_id}</td>
                    <td class="mono text-xs">
                      {proposal.from_path} → {proposal.to_path}
                    </td>
                    <td>
                      <span
                        class={`badge badge-sm ${
                          proposal.status === 'pending'
                            ? 'badge-warning'
                            : proposal.status === 'accepted'
                              ? 'badge-success'
                              : 'badge-error'
                        }`}
                      >
                        {proposal.status}
                      </span>
                    </td>
                    <td class="mono text-sm max-w-xs truncate">
                      {proposal.title || '(no title)'}
                    </td>
                    <td>
                      <button
                        class="btn btn-ghost btn-xs"
                        onClick={() => onViewDetails(proposal)}
                        disabled={submitting}
                      >
                        Details
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}
