import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
    formatReferralReward,
    referralProgramLabel,
    referralCapacity,
    type ReferralOffer,
    type ReferralProgram,
} from './referral';
import './ReferralQuotaCard.css';

export function ReferralQuotaCard({ accountId, program }: { accountId: string; program: ReferralProgram }) {
    const [offer, setOffer] = useState<ReferralOffer | null>(null);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState('');
    const generation = useRef(0);

    const refresh = useCallback(async () => {
        const generationId = ++generation.current;
        setLoading(true);
        setError('');
        try {
            const data = await invoke<ReferralOffer>('get_desktop_referral_eligibility', {
                id: accountId,
                program,
            });
            if (generationId === generation.current) setOffer(data);
        } catch (err) {
            if (generationId === generation.current) {
                setOffer(null);
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

    const capacity = referralCapacity(offer);
    const showOffer = offer?.should_show === true;

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
            </div>

            {loading && !offer && <p className="referral-quota-muted" role="status">正在查询邀请资格…</p>}
            {error && (
                <div className="referral-quota-error" role="alert">
                    <span>{error}</span>
                    <button className="referral-quota-retry" onClick={() => void refresh()}>重试</button>
                </div>
            )}
            {offer && !error && (
                showOffer ? (
                    <>
                        <div className="referral-quota-stats">
                            <div>
                                <span className="referral-quota-label">每位奖励</span>
                                <strong>{formatReferralReward(offer)}</strong>
                            </div>
                            <div className="referral-quota-remaining">
                                <span className="referral-quota-label">还能邀请</span>
                                <strong>{capacity} 人</strong>
                            </div>
                        </div>
                        <div className="referral-quota-breakdown">
                            <span>发送上限 {offer.remaining_send_capacity ?? '未提供'}</span>
                            <span>奖励名额 {offer.remaining_reward_capacity ?? '未提供'}</span>
                        </div>
                        <p className="referral-quota-note">
                            活动奖励不等于当前余额；对方接受邀请并完成官方要求后才会到账。
                        </p>
                    </>
                ) : (
                    <p className="referral-quota-muted">当前账号暂未开放此邀请活动。</p>
                )
            )}
        </section>
    );
}
