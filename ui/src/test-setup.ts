// jsdom does not implement <dialog>.showModal()/close(); the aesthetic
// .ae-dialog confirm flow uses them, so polyfill the pair for tests.
if (typeof HTMLDialogElement !== "undefined") {
  if (!HTMLDialogElement.prototype.showModal) {
    HTMLDialogElement.prototype.showModal = function showModal() {
      this.open = true;
    };
  }
  if (!HTMLDialogElement.prototype.close) {
    HTMLDialogElement.prototype.close = function close(returnValue?: string) {
      this.open = false;
      if (typeof returnValue === "string") this.returnValue = returnValue;
      this.dispatchEvent(new Event("close"));
    };
  }
}
