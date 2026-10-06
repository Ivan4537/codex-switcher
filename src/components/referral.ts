export interface ReferralGrant {
    recipient?: string;
    grant_type?: string;
    amount?: number;
}

export interface ReferralOffer {
    query_source?: 'verified_browser';
    should_show?: boolean;
    offer_id?: string | null;
    grants?: ReferralGrant[];
    remaining_send_capacity?: number;
    remaining_reward_capacity?: number;
    requires_explicit_confirmation?: boolean;
}

export function parseReferralOffer(value: unknown): ReferralOffer {
    const offer = value as ReferralOffer | null;
    if (!offer || typeof offer !== 'object' || typeof offer.should_show !== 'boolean'
        || (offer.should_show && (!Number.isInteger(offer.remaining_send_capacity) || (offer.remaining_send_capacity ?? -1) < 0))
        || (offer.grants != null && (!Array.isArray(offer.grants) || offer.grants.some(grant => !grant || typeof grant !== 'object')))
        || (offer.remaining_reward_capacity != null && (!Number.isInteger(offer.remaining_reward_capacity) || offer.remaining_reward_capacity < 0))
        || (offer.offer_id != null && typeof offer.offer_id !== 'string')) {
        throw new Error('邀请资格响应不完整，活动和剩余次数未确认，请重试或在官方 Desktop 查看。');
    }
    return offer;
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

/** 活动展示的单人奖励；没有真实 grants 时不根据 offer_id 猜测金额。 */
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
    return '活动未提供奖励数额';
}

/** 接口真实返回的奖励金额是否可确认。 */
export function hasKnownReferralReward(offer: { grants?: ReferralGrant[] } | null): boolean {
    return (offer?.grants ?? []).some(
        grant => grant.recipient === 'referrer' && (grant.amount ?? 0) > 0,
    );
}

/** 当前请求最多可发送的邮箱数量；不等同于奖励名额。 */
export function referralSendCapacity(offer: ReferralOffer | null): number {
    if (offer?.should_show !== true) return 0;
    let capacity = Math.min(5, offer.remaining_send_capacity ?? 0);
    const grants = (offer.grants ?? []).length > 0;
    const legacyOffer = !!offer.offer_id && offer.offer_id !== 'none';
    if (grants || legacyOffer) {
        capacity = Math.min(capacity, Math.max(0, offer.remaining_reward_capacity ?? 0));
    }
    return Math.max(0, capacity);
}

/** 奖励名额是否明确为 0；null 表示接口没有提供该字段。 */
export function referralRewardCapacity(offer: ReferralOffer | null): number | null {
    if (!offer || typeof offer.remaining_reward_capacity !== 'number') return null;
    return Math.max(0, offer.remaining_reward_capacity);
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
