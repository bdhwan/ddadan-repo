export type ScanState =
  | "READY"
  | "SCANNING"
  | "CUSTOMER_RESOLVED"
  | "INPUT"
  | "REVIEW"
  | "SUBMITTING"
  | "SUCCESS"
  | "FAILURE";
export type CameraState =
  | "unchecked"
  | "checking"
  | "ready"
  | "denied"
  | "unavailable"
  | "insecure";

const TRANSITIONS: Readonly<Record<ScanState, readonly ScanState[]>> = {
  READY: ["SCANNING"],
  SCANNING: ["CUSTOMER_RESOLVED"],
  CUSTOMER_RESOLVED: ["INPUT"],
  INPUT: ["REVIEW"],
  REVIEW: ["INPUT", "SUBMITTING"],
  SUBMITTING: ["SUCCESS", "FAILURE"],
  SUCCESS: ["READY"],
  FAILURE: ["READY", "SUBMITTING"],
};

export class ScanStateMachine {
  private _state: ScanState = "READY";
  private _camera: CameraState = "unchecked";
  private _frameLocked = false;

  get state(): ScanState {
    return this._state;
  }
  get camera(): CameraState {
    return this._camera;
  }
  get frameLocked(): boolean {
    return this._frameLocked;
  }

  checkingCamera(): void {
    this._camera = "checking";
  }
  cameraReady(): void {
    this._camera = "ready";
  }
  cameraDenied(): void {
    this._camera = "denied";
    this._frameLocked = false;
  }
  cameraUnavailable(): void {
    this._camera = "unavailable";
    this._frameLocked = false;
  }
  insecureContext(): void {
    this._camera = "insecure";
    this._frameLocked = false;
  }

  startScanning(): void {
    this.move("SCANNING");
    this._frameLocked = false;
  }
  lockDecodedFrame(): boolean {
    if (this._state !== "SCANNING" || this._frameLocked) return false;
    this._frameLocked = true;
    return true;
  }
  rejectDecodedFrame(): void {
    if (this._state !== "SCANNING")
      throw new Error("Decoded frames can only be rejected while scanning");
    this._frameLocked = false;
  }
  customerResolved(): void {
    this.move("CUSTOMER_RESOLVED");
  }
  beginInput(): void {
    this.move("INPUT");
  }
  review(): void {
    this.move("REVIEW");
  }
  editInput(): void {
    this.move("INPUT");
  }
  submit(): void {
    this.move("SUBMITTING");
  }
  succeed(): void {
    this.move("SUCCESS");
  }
  fail(): void {
    this.move("FAILURE");
  }
  retryUncertainSubmission(): void {
    this.move("SUBMITTING");
  }
  nextCustomer(): void {
    this.move("READY");
    this._frameLocked = false;
  }

  private move(next: ScanState): void {
    if (!TRANSITIONS[this._state].includes(next))
      throw new Error(`Invalid scan transition ${this._state} -> ${next}`);
    this._state = next;
  }
}
