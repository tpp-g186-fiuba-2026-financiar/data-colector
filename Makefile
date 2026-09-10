dev:
	docker compose up --build

dev-d:
	docker compose up --build -d
	
stop:
	docker compose down

network:
	docker network create financiar_shared_network