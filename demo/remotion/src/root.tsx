import React from "react";
import { Composition } from "remotion";
import { CurbDemo } from "./CurbDemo";

export function RemotionRoot() {
  return (
    <Composition
      id="CurbDemo"
      component={CurbDemo}
      durationInFrames={900}
      fps={30}
      width={1920}
      height={1080}
    />
  );
}
