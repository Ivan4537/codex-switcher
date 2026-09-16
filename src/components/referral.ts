export interface ReferralGrant {
    recipient?: string;
    grant_type?: string;
    amount?: number;
}

export interface ReferralOffer {
    should_show?: boolean;
    offer_id?: string | null;
    grants?: ReferralGrant[];
    remaining_send_capacity?: number;
    remaining_reward_capacity?: number;
    requires_explicit_confirmation?: boolean;
}

export type ReferralProgram = 'codex_referral_consumer' | 'codex_referral_workspace';

/** 只为当前已知支持邀请活动的套餐返回对应 program；未知/免费套餐保持隐藏。 */
export function referralProgramForPlan(plan?: string | null): ReferralProgram | null {
    const normalized = (plan ?? '').trim().toLowerCase();
    if (normalized === 'plus' || normalized === 'pro') return 'codex_referral_consumer';
    if (['team', 'business', 'enterprise', 'edu'].includes(normalized)) {
        return 'codex_referral_workspace';
    }
    return null;
}

export function referralProgramLabel(program: ReferralProgram): '个人活动' | '工作区活动' {
    return program === 'codex_referral_workspace' ? '工作区活动' : '个人活动';
}

export interface ReferralInvite {
    email?: string;
    referral_id?: string;
    invite_url?: string;
    status?: string;
}

/** 活动展示的单人奖励；只有上游未返回 grants 时才使用已知 offer_id 兜底。 */
export function formatReferralReward(offer: ReferralOffer): string {
    const grants = (offer.grants ?? []).filter(
        grant => grant.recipient === 'referrer' && (grant.amount ?? 0) > 0,
    );
    if (grants.length) {
        return grants.map(grant => {
            const unit = grant.grant_type === 'personal_credits' ? '个使用额度'
                : grant.grant_type === 'workspace_credits' ? '个工作区额度'
                    : grant.grant_type?.includes('rate_limit_reset') ? '次限额重置'
                        : `（${grant.grant_type ?? '奖励'}）`;
            return `${grant.amount?.toLocaleString()} ${unit}`;
        }).join(' + ');
    }
    if (!offer.grants?.length) {
        const amount = { credits_250: 250, credits_500: 500, credits_1000: 1000 }[offer.offer_id ?? ''];
        if (amount) return `${amount.toLocaleString()} 个使用额度`;
    }
    return '活动未提供奖励数额';
}

/** 可实际发送且仍有机会获得活动奖励的邮箱数量。 */
export function referralCapacity(offer: ReferralOffer | null): number {
    if (!offer?.should_show) return 0;
    let capacity = Math.min(5, offer.remaining_send_capacity ?? 0);
    if (offer.grants?.length || (offer.offer_id != null && offer.offer_id !== 'none')) {
        capacity = Math.min(capacity, offer.remaining_reward_capacity ?? 0);
    }
    return Math.max(0, capacity);
}

export interface ReferralTracking {
    items: ReferralInvite[];
    cursor?: string | null;
}

export interface ReferralSendResult {
    invites: ReferralInvite[];
    failed_emails?: string[];
    grants?: ReferralGrant[];
    offer_id?: string;
    message?: string;
}
