# Alpine Rush — démo Void

Mini-jeu de course arcade en minecart, **solo ou jusqu'à huit joueurs**, pour
**Minecraft Java 26.1.2**, sans mod ni resource pack. Le véhicule se pilote librement
sur une chaussée : ses déplacements sont simulés par le serveur.

> **État du portage.** Cette version couvre le socle (D0) du portage de la démo sur les
> API actuelles du moteur — génération du monde, géométrie du circuit et simulation
> pure des karts —, la machine à états de la course (D1) : phases, commandes,
> annonces et bossbars, et les karts en tant qu'entités du moteur (D2) : minecart
> monté, pilotage au clavier et déplacement piloté par le serveur. Le reste —
> cristaux, bonus, affichages et barrière de téléportation — est **en cours de
> portage** et arrive dans les unités suivantes.

## Lancer

Depuis la racine du dépôt :

```sh
cargo run --release -p voidmc-demo
```

La démo écoute par défaut sur toutes les interfaces IPv4 (`0.0.0.0:25565`).
Se connecter à `127.0.0.1:25565` depuis la machine hôte, ou à son adresse IP depuis
une autre machine. Le joueur apparaît à **Y = 110**, au-dessus de la vallée, assis
dans son minecart d'attente ; le vol libre des spectateurs revient avec l'unité
travel (D5). Aucune plateforme ne gêne la vue du paysage.

Pour utiliser une autre carte :

```sh
VOID_DEMO_SEED=2026 cargo run --release -p voidmc-demo
```

`VOID_DEMO_ADDRESS` permet de modifier l'adresse et le port d'écoute. La seed est un
entier non signé sur 64 bits ; sa valeur par défaut est `42`. Le serveur tourne
à 20 ticks/s. `RUST_LOG` règle la verbosité des logs.

## Jouer

Chaque pilote inscrit est assis dans **un minecart** qui lui appartient (`src/vehicle.rs`).
Le minecart est une entité du moteur : le serveur le fait apparaître chez les joueurs
qui ont chargé son chunk, le retire quand ils s'éloignent ou se déconnectent, et
transmet ses déplacements. Le pilote en est le passager. Sur une pression de Sneak,
le serveur renvoie la liste des passagers du kart (fidèle à la référence) — uniquement
sur cette pression, jamais périodiquement ; comme la descente d'un véhicule est
décidée côté serveur en 26.1.2 et que ce moteur ne l'implémente pas, le client ne
quitte jamais son kart de lui-même.

Le pilotage utilise les touches de déplacement du client (`ServerboundPlayerInput`) :
**Avancer** accélère, **Reculer** freine puis passe en marche arrière, **Gauche/Droite**
tournent, **Saut** enclenche le boost (tant qu'il reste du carburant), **Sprint**
utilisera le bonus ramassé (D3). Les règles de conduite (accélération, traînée,
boost rechargeable, rebonds sur les glissières, chocs entre minecarts) sont celles de
`src/kart.rs` ; elles ne s'appliquent que pendant la course. À chaque tick, le
serveur déplace l'entité minecart vers la position simulée ; le moteur choisit
lui-même entre un déplacement relatif et une téléportation (au-delà de 8 blocs, par
exemple lors de la mise en grille). Un kart immobile ne génère aucun paquet.

Un client assis dans un minecart ne rapporte plus sa position (il n'envoie que sa
rotation) : `pose` écrit donc aussi la `Position` du pilote, 0,35 bloc au-dessus du
kart comme dans la référence, pour que le chargement des chunks et la position vue
par les autres joueurs suivent le kart pendant toute la course.

La manche est complète (`src/race.rs`) :

- `/race [tours]` accepte **1 à 20 tours** (3 par défaut). Le nombre choisi vaut pour
  tous les pilotes de la manche ; une commande invalide ne lance pas de course, et une
  manche en cours ne peut pas être relancée.
- Chaque manche construit un **nouveau tracé** dans la vallée, un chunk tous les deux
  ticks, avec une annonce à 25/50/75 %. La grille est placée dès que le circuit est
  prêt ; le chargement des chunks chez chaque pilote et la barrière de téléportation
  reviennent avec l'unité travel (D5), le compte à rebours de cinq secondes démarre
  donc immédiatement.
- Le nombre de tours choisi, avec huit checkpoints par tour, à franchir dans l'ordre.
  Le chat annonce chaque nouveau tour au pilote concerné, les arrivées, puis le podium.
  La limite de temps est de dix minutes jusqu'à trois tours, puis de 200 secondes par
  tour ; le chat prévient à 60, 30 et 10 secondes de la fin.
- La **barre d'action** des pilotes affiche la phase, puis tour, checkpoint, temps,
  pénalités, vitesse et bonus pendant la course. La **bossbar bleue** de chaque pilote
  affiche sa réserve de boost ; une bossbar partagée suit la construction, le compte
  à rebours et le temps restant.
- `/reset` ramène au dernier checkpoint avec trois secondes de pénalité ; le minecart
  y est téléporté par le moteur.
- `/scores` affiche les temps de la manche et les meilleurs temps du circuit actuel
  pour le même nombre de tours. Les records sont en mémoire, par seed de circuit,
  nombre de tours et nom de joueur, et disparaissent au redémarrage.
- `/leave` passe en spectateur ; `/join` inscrit pour le prochain départ. Chaque joueur
  qui arrive est inscrit automatiquement (huit pilotes au maximum). Une arrivée ou un
  retour pendant une manche attend la suivante ; un pilote qui se déconnecte pendant
  la course abandonne la manche.
- Lorsque tous les participants ont terminé ou quitté, le circuit se démonte dans
  l'ordre inverse et le terrain initial est restauré ; `/race` redevient disponible
  à la fin du démontage.

### Power-ups et effets

*En cours de portage (D3).* Les huit bonus — Turbo, Bouclier, Banane, Missile guidé,
Onde de choc, Nappe de glace, Éclair, Super-recharge — et leurs effets sur le
véhicule (durées, ralentissements, dérapages, absorption par le bouclier) sont déjà
définis dans `src/kart.rs` (`PowerUp`, `Kart::activate`, `Kart::strike`). Les
cristaux flottants, pièges, projectiles et particules suivront.

### Visuels et animation

*En cours de portage (D4).*

## Carte procédurale

Le **décor permanent** utilise plusieurs échelles de bruit pour ses vallées, lacs,
berges de sable et gravier, crêtes rocheuses et sommets enneigés. Des sapins de tailles
variées, des rochers moussus, des fougères et des fleurs complètent les versants.
Les décorations traversent les frontières de chunks. L'attente se fait en vol libre
au-dessus du décor naturel, sans construction de lobby.

Le **circuit de chaque manche** est une spline fermée traversant **10 à 14 points de
passage tirés aléatoirement** : distances au centre, angles, orientation, étirement
et altitudes changent. La route, large d'environ quatorze blocs, dispose d'un
revêtement gris texturé, d'un marquage central discontinu, de vibreurs cyan/orange et
blancs, de parapets vitrés, de supports de viaduc et de portiques éclairés. Les huit
checkpoints sont espacés selon la **distance parcourue sur la courbe**, commune au
pilotage et à la génération.

`VOID_DEMO_SEED` fixe le décor et la séquence des seeds de manches ; redémarrer avec
la même valeur reproduit cette séquence. Tous les bits de la seed participent à la
génération. Le décor reste identique entre les manches.

Pour une présentation, une distance d'affichage client de 12 chunks permet de
profiter du paysage. Seuls les chunks traversés par le circuit sont mis à jour lors
de la construction ; le démontage suit l'ordre inverse et restaure exactement le
terrain initial, y compris les arbres dégagés pour la piste. Le profil `--release`
est recommandé.

## Organisation technique

- `src/terrain.rs` : paysage permanent, bruit de relief et décoration déterministe
  (`Alpine`, générateur `WorldGenerator`, biome taïga).
- `src/track.rs` : spline procédurale, index spatial par chunk, projection sur le
  tracé, distances de course et superposition des blocs du circuit (`Track`, `GATES`,
  `HALF_WIDTH`).
- `src/arena.rs` : générateur partagé avec le streaming (`Arena`, `WAIT_Y`) ;
  `Arena::replace` remplace les données ECS d'un chunk et le renvoie aux joueurs qui
  l'ont chargé via `WorldPlayers::broadcast_chunk`. Un circuit démonté ne réapparaît
  pas lors d'un chargement ultérieur.
- `src/kart.rs` : simulation pure, sans réseau — `Input`, `Kart` (grille, direction,
  accélération, traînée, glissières, `collide`, `crosses_gate`, `progress`),
  `PowerUp` et `Strike` (effets des bonus sur le véhicule). Les constantes numériques
  sont celles de la référence ; le yaw du modèle de minecart est décalé de +90°.
- `src/vehicle.rs` : le kart comme entité du moteur — `spawn` (`EntityBuilder`
  minecart + `Kart` + `Pilot` + `Passengers`), `Karts` (accès au kart d'un joueur via
  `Racer::kart`), l'observateur `input` (`PlayerInputEvent` → `Kart::input`,
  renvoi des passagers sur Sneak), le système `drive` (`Kart::drive` puis `collide`
  pendant la course) et `pose`, qui ne réécrit `Position`/`Rotation` du kart et la
  `Position` du pilote (`SEAT_HEIGHT` au-dessus) que si la simulation a bougé le kart.
- `src/race.rs` : phases, commandes, annonces, bossbars et HUD ; `Racer` relie le
  joueur à son kart.
- `src/main.rs` : configuration par variables d'environnement et démarrage du serveur.

## Vérifications

```sh
cargo test -p voidmc-demo
```

Les tests couvrent la génération déterministe (paysage et arène), la diversité des
seeds, la continuité de 65 tracés, le parcours de trois tours sur plusieurs
géométries, les checkpoints ordonnés, la marche arrière, les rebonds, les bumps, les
boucliers, les bonus à usage unique et leurs effets, la préservation du paysage
pendant la construction et la restauration exacte au démontage. Les tests d'`App`
sans réseau vérifient l'inscription (un seul minecart par pilote, passager compris),
le renvoi unique des passagers par pression de Sneak, le déplacement de l'entité et
du pilote pendant la course (et l'absence de tout paquet ou écriture pour un kart
immobile), la disparition du kart
au départ du joueur et le choc entre deux karts, identique à la simulation pure.
