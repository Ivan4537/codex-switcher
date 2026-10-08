import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
    formatReferralReward,
    hasKnownReferralReward,
    referralProgramLabel,
    referralRewardCapacity,
    referralSendCapacity,
    parseReferralOffer,
    type ReferralOffer,
    type ReferralProgram,
} from './referral';
import './ReferralQuotaCard.css';

export function ReferralQuotaCard({ accountId, program }: { accountId: string; program: ReferralProgram }) {
    const [offer, setOffer] = useState<ReferralOffer | null>(null);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState('');
    const generation = useRef(0);
    const [offerKey, setOfferKey] = useState('');
    const key = accountId + ':' + program;

    const refresh = useCallback(async (browser = false) => {
        const generationId = ++generation.current;
        setLoading(true);
        setError('');
        try {
            const data = await invoke<ReferralOffer>(browser ? 'get_desktop_referral_eligibility_browser' : 'get_desktop_referral_eligibility', {
                id: accountId,
                program,
            });
            if (generationId === generation.current) { setOffer(parseReferralOffer(data)); setOfferKey(accountId + ':' + program); }
        } catch (err) {
            if (generationId === generation.current) {
                setError(String(err));
            }
        } finally {
            if (generationId === generation.current) setLoading(false);
        }
    }, [accountId, program]);

    useEffect(() => {
        void refresh();
        return () => { generation.current++; };
    }, [refresh]);

    const currentOffer = offerKey === key ? offer : null;
    const capacity = referralSendCapacity(currentOffer);
    const rewardCapacity = referralRewardCapacity(currentOffer);
    const knownReward = hasKnownReferralReward(currentOffer);
    const showOffer = currentOffer?.should_show === true;

    return (
        <section className="referral-quota-card" aria-label="邀请额度">
            <div className="referral-quota-header">
                <div>
                    <strong>邀请额度</strong>
                    <span>{referralProgramLabel(program)}</span>
                </div>
                <button
                    className="referral-quota-refresh"
                    onClick={() => void refresh()}
                    disabled={loading}
                    title="刷新邀请额度"
                >
                    {loading ? '查询中…' : '刷新'}
                </button>
                <button className="referral-quota-refresh" disabled={loading} onClick={() => void refresh(true)}>在浏览器会话中查询</button>
            </div>

            {loading && <p className="referral-quota-muted" role="status">正在查询邀请资格…</p>}
            {error && (
                <div className="referral-quota-error" role="alert">
                    <strong>邀请资格未确认</strong>
                    <span>{error}</span>
                    <button className="referral-quota-retry" onClick={() => void refresh()}>重试</button>
                    <button className="referral-quota-retry" onClick={() => void invoke('open_official_referral_client').catch(err => setError(String(err)))}>在官方 Desktop 查看</button>
                </div>
            )}
            {currentOffer && showOffer && error && <p className="referral-quota-muted">上次成功查询：剩余邀请次数 {currentOffer.remaining_send_capacity}。数据已过期，请以官方 Desktop 为准。</p>}
            {currentOffer && !error && !loading && (
                showOffer ? (
                    <>
                        <div className="referral-quota-stats">
                            <div>
                                <span className="referral-quota-label">每位奖励</span>
                                <strong>{formatReferralReward(currentOffer)}</strong>
                            </div>
                            <div className="referral-quota-remaining">
                                <span className="referral-quota-label">剩余邀请次数</span>
                                <strong>{currentOffer.remaining_send_capacity}</strong>
                            </div>
                        </div>
                        <div className="referral-quota-breakdown">
                            <span>发送上限 {currentOffer.remaining_send_capacity ?? '未提供'}</span>
                            <span>奖励名额 {currentOffer.remaining_reward_capacity ?? '未提供'}</span>
                            <span>单次最多 {capacity} 人</span>
                        </div>
                        <p className="referral-quota-note">
                            {currentOffer.query_source === 'verified_browser' && '浏览器已确认资格和次数；发送邀请请在官方 Desktop 完成。'}
                            {!knownReward
                                ? '接口未提供实际奖励金额，不能根据活动编号推断奖励数额。'
                                : '活动奖励不等于当前余额；对方接受邀请并完成官方要求后才会到账。'}
                            {rewardCapacity === 0 && (capacity > 0 ? ' 当前没有奖励名额，仍可发送无奖励邀请。' : ' 当前活动没有可发送名额。')}
                        </p>
                        {currentOffer.query_source === 'verified_browser' && <button className="referral-quota-retry" onClick={() => void invoke('open_official_referral_client').catch(err => setError(String(err)))}>在官方 Desktop 查看</button>}
                    </>
                ) : (
                    <p className="referral-quota-muted">本次查询未显示邀请入口，不代表邀请次数为零。{currentOffer?.query_source === 'verified_browser' ? ` 已查询 ${(currentOffer.checked_entrypoints ?? ['persistent']).join(' / ')}。` : ' 请在浏览器会话中补查官方邀请入口。'}</p>
                )
            )}
        </section>
    );
}
