# Alpine Rush — démo Void

Mini-jeu de course arcade en minecart, **solo ou jusqu'à huit joueurs**, pour
**Minecraft Java 26.1.2**, sans mod ni resource pack. Le véhicule se pilote librement
sur une chaussée : ses déplacements sont simulés par le serveur.

> **État du portage.** Cette version est le socle (D0) du portage de la démo sur les
> API actuelles du moteur : génération du monde, géométrie du circuit et simulation
> pure des karts. Le reste — machine à états de la course, commandes, karts en tant
> qu'entités, cristaux, bonus, affichages et téléportation — est **en cours de
> portage** et arrive dans les unités suivantes.

## Lancer

Depuis la racine du dépôt :

```sh
cargo run --release -p voidmc-demo
```

La démo écoute par défaut sur toutes les interfaces IPv4 (`0.0.0.0:25565`).
Se connecter à `127.0.0.1:25565` depuis la machine hôte, ou à son adresse IP depuis
une autre machine. Le joueur apparaît à **Y = 110**, au-dessus de la vallée, et
retombe sur le relief ; le vol libre revient avec l'unité travel (D5). Aucune
plateforme ne gêne la vue du paysage.

Pour utiliser une autre carte :

```sh
VOID_DEMO_SEED=2026 cargo run --release -p voidmc-demo
```

`VOID_DEMO_ADDRESS` permet de modifier l'adresse et le port d'écoute. La seed est un
entier non signé sur 64 bits ; sa valeur par défaut est `42`. Le serveur tourne
à 20 ticks/s. `RUST_LOG` règle la verbosité des logs.

## Jouer

*En cours de portage.* Les commandes `/race`, `/reset`, `/join`, `/leave` et
`/scores`, le pilotage, les checkpoints, la bossbar de boost, le classement et les
annonces reviennent avec les unités D1 et D2. Les règles de pilotage (accélération,
freinage, marche arrière, boost rechargeable, rebonds sur les glissières, chocs entre
minecarts) sont déjà implémentées et testées dans `src/kart.rs`.

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
- `src/main.rs` : configuration par variables d'environnement et démarrage du serveur.

## Vérifications

```sh
cargo test -p voidmc-demo
```

Les tests couvrent la génération déterministe (paysage et arène), la diversité des
seeds, la continuité de 65 tracés, le parcours de trois tours sur plusieurs
géométries, les checkpoints ordonnés, la marche arrière, les rebonds, les bumps, les
boucliers, les bonus à usage unique et leurs effets, la préservation du paysage
pendant la construction et la restauration exacte au démontage.
