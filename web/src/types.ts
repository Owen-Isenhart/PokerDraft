export type Player = {
  id: string;
  name: string;
  house: boolean;
  balance: number;
  score: number;
  cards: number[];
};
export type Rules = {
  seats: number;
  auction_ms: number;
  pricing: "first_price" | "second_price";
};
export type Snapshot = {
  rules_version: string;
  rules: Rules;
  round: number;
  card: number | null;
  players: Player[];
  pot: number;
  finished: boolean;
  commitment: string;
  seed: string | null;
  seq: number;
};
export type GameEvent = {
  seq: number;
  round: number;
  kind: string;
  data: Record<string, unknown>;
};
export type Match = {
  id: string;
  status: string;
  deadline_ms?: number;
  server_time_ms?: number;
  snapshot: Snapshot;
  events: GameEvent[];
  history?: Snapshot[];
  created_ms?: number;
};
export type Bot = {
  id: string;
  name: string;
  team_id: string;
  team: string;
  rating: number;
  games: number;
  disabled: boolean;
};
export type Summary = {
  id: string;
  round: number;
  status?: string;
  players: Player[];
  created_ms?: number;
  tournament?: string;
};
export type Directory = {
  type: string;
  protocol: number;
  server_time_ms: number;
  rules: Rules;
  paused: boolean;
  lobby: { bots: Bot[]; deadline_ms: number };
  matches: Summary[];
  teams: number;
  registered_bots: number;
};
export type Pairing = {
  id: number;
  a: string;
  b: string | null;
  winner: string | null;
  match_id: string | null;
  wins: [number, number];
};
export type Tournament = {
  id: string;
  name: string;
  state: string;
  swiss_rounds: number;
  cut: number;
  round: number;
  entrants: {
    bot: string;
    points: number;
    opponents: string[];
    byes: number;
    seed: number;
  }[];
  pairings: Pairing[];
  history: Pairing[];
  champion: string | null;
};
export type Standings = {
  id: string;
  entries: { bot: string; points: number; buchholz: number; byes: number }[];
};
