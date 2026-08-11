export type WithdrawalStep =
  | "IMPACT"
  | "REAUTHENTICATION"
  | "SUBMITTING"
  | "COMPLETE";

export interface WithdrawalFlowState {
  step: WithdrawalStep;
  impact_acknowledged: boolean;
  reauthenticated: boolean;
}

export const initialWithdrawalFlow: WithdrawalFlowState = {
  step: "IMPACT",
  impact_acknowledged: false,
  reauthenticated: false,
};

export function acknowledgeWithdrawalImpact(): WithdrawalFlowState {
  return {
    step: "REAUTHENTICATION",
    impact_acknowledged: true,
    reauthenticated: false,
  };
}

export function completeWithdrawalReauthentication(
  state: WithdrawalFlowState,
): WithdrawalFlowState {
  if (!state.impact_acknowledged) return state;
  return { ...state, reauthenticated: true, step: "SUBMITTING" };
}

export function completeWithdrawal(
  state: WithdrawalFlowState,
): WithdrawalFlowState {
  if (!state.reauthenticated) return state;
  return { ...state, step: "COMPLETE" };
}
