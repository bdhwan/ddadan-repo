import { describe, expect, it } from 'vitest';
import { ScanStateMachine } from './scan-state-machine';

describe('store scan state machine', () => {
  it('follows the complete required success path and waits for next customer', () => {
    const machine = new ScanStateMachine();
    expect(machine.state).toBe('READY');
    machine.startScanning(); expect(machine.state).toBe('SCANNING');
    expect(machine.lockDecodedFrame()).toBe(true);
    expect(machine.lockDecodedFrame()).toBe(false);
    machine.customerResolved(); expect(machine.state).toBe('CUSTOMER_RESOLVED');
    machine.beginInput(); machine.review(); machine.submit(); machine.succeed();
    expect(machine.state).toBe('SUCCESS');
    expect(() => machine.startScanning()).toThrow(/Invalid scan transition/);
    machine.nextCustomer(); expect(machine.state).toBe('READY');
  });

  it('moves submitting to failure and retries an uncertain result without resetting', () => {
    const machine = reachReview();
    machine.submit(); machine.fail(); expect(machine.state).toBe('FAILURE');
    machine.retryUncertainSubmission(); expect(machine.state).toBe('SUBMITTING');
    machine.succeed(); expect(machine.state).toBe('SUCCESS');
  });

  it('supports camera denial, manual fallback, and camera permission recovery', () => {
    const machine = new ScanStateMachine();
    machine.checkingCamera(); machine.cameraDenied();
    expect(machine.camera).toBe('denied');
    machine.startScanning(); // manual 8-digit code resolve follows the same state path
    machine.lockDecodedFrame(); machine.customerResolved(); machine.beginInput(); machine.review(); machine.submit(); machine.fail(); machine.nextCustomer();
    machine.checkingCamera(); machine.cameraReady(); machine.startScanning();
    expect(machine.camera).toBe('ready');
    expect(machine.state).toBe('SCANNING');
  });
});

function reachReview(): ScanStateMachine {
  const machine = new ScanStateMachine();
  machine.startScanning(); machine.lockDecodedFrame(); machine.customerResolved(); machine.beginInput(); machine.review();
  return machine;
}
