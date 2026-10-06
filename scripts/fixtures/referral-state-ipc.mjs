export const mock = window.__referralMock = { mode: 'blocked', calls: [] };
export async function invoke(command, args) {
    mock.calls.push({ command, args });
    if (command === 'get_desktop_referral_eligibility' || command === 'get_desktop_referral_eligibility_browser') {
        if (command === 'get_desktop_referral_eligibility_browser') return {
            should_show: true, remaining_send_capacity: 3, remaining_reward_capacity: 0,
            grants: [], offer_id: 'none', query_source: 'verified_browser',
        };
        if (mock.mode === 'blocked') throw '邀请查询被网页防护拦截，资格和剩余次数未确认；这不代表没有活动。';
        if (mock.mode === 'incomplete') return {};
        if (mock.mode === 'inactive') return { should_show: false };
        return { should_show: true, remaining_send_capacity: 3, remaining_reward_capacity: 0,
            grants: [], offer_id: 'none', requires_explicit_confirmation: true };
    }
    throw new Error('Fixture denies mutation or unmocked IPC: ' + command);
}
