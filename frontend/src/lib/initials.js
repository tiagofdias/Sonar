export const initials = (name) => name?.trim().split(/\s+/).map((s) => s[0]).slice(0, 2).join("").toUpperCase() || "?";
