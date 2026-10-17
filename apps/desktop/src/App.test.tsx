import { render, screen } from "@testing-library/react";
import App from "./App";

test("renders the Sigrun heading", () => {
  render(<App />);
  expect(screen.getByRole("heading", { name: "Sigrun" })).toBeTruthy();
});
