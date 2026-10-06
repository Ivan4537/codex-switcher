// Used only by the isolated Vite acceptance server; no production IPC.
export const mock = window.__anchorMock = { blockedTarget: true, calls: [] };
export async function invoke(command, args) {
    mock.calls.push({ command, args });
    if (command === 'get_quota_by_id') return {
        plan_type: 'business', five_hour_left: 100, weekly_left: 100,
        desktop_gate: args.id === 'anchor'
            ? { allowed: false, limit_reached: true, reason: 'workspace_owner_credits_depleted' }
            : { allowed: true, limit_reached: false, reason: null },
    };
    if (command === 'recover_session_anchor') {
        if (mock.blockedTarget) throw 'Target workspace is blocked. No anchor was changed.';
        window.dispatchEvent(new CustomEvent('anchor-mock-migrated', { detail: args.target }));
        return null;
    }
    throw new Error('Unmocked IPC denied: ' + command);
}
