# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Architecture

This repository contains the backend API for the Occam service. Occam is a web application that enables customers to find the most cost-effective subset of streaming services to watch all the content they want. 

There are three core services provided by the API:
1. **Title Search** - Customers must be able to search for and select titles (movies, TV shows) from a drop-down list such that plain-english titles can be translated to uniform media identifiers.
2. **Optimization Query** - Customers must be able to find the best subset of streaming services to view their favorite movies and TV shows. Customers can select "must have" and "nice to have" titles. The optimizer must only return combinations that have ALL "must have" titles, and return the most cost effective option that has the MOST "nice to have" options (prioritizing cost over nice to have, but showing alternative options).
3. **Recommendation Engine** - Given the preferred list of titles and the optimal subset, customers should be able to receive personalized watch recommendations available on the subset of services they are subscribed to.

## Data Source

A third-party data source should be used to retrieve title information. Caching should be used to optimize response time and minimize API costs.

## Documentation

### README Maintenance

The README.md file is the primary source of truth for understanding the system architecture, data flows, and implementation details. It must be kept up-to-date as the codebase evolves.

**IMPORTANT: Before starting any work session:**
1. Read the README.md file to understand the current system architecture and implementation status
2. Use the README as your primary reference for understanding how components work together

**When making major changes, you MUST update the README.md:**

Major changes include:
- Adding, modifying, or removing API endpoints
- Changing the database schema or caching strategy
- Implementing or modifying core service logic (title search, optimization, recommendations)
- Adding new dependencies or changing the tech stack
- Modifying the data flow or architecture
- Changing configuration requirements
- Adding new features or capabilities

**How to update the README:**
1. Read the current README.md before making changes
2. After implementing changes, update the relevant sections:
   - Architecture diagrams and descriptions
   - Data flow explanations
   - API endpoint documentation
   - Configuration requirements
   - Project structure if files were added/removed
   - Next Steps section if implementation status changed
3. Ensure all code examples and API signatures in the README match the actual implementation
4. Update the "Next Steps" section to reflect what's been completed and what remains

**README Sections to maintain:**
- **Architecture**: System overview and component interactions
- **Data Flow**: Detailed explanation of how each service processes requests
- **API Endpoints**: Request/response examples for all endpoints
- **Caching Strategy**: Current Redis and PostgreSQL usage patterns
- **Project Structure**: File organization and module purposes
- **Next Steps**: Current implementation status and remaining work

The README should always be self-documenting - anyone reading it should understand the system without needing to ask questions or read the code.

## Code Style
- Always keep code simple and maintainable. Readability and testability is critical.
- Document methods with solid -- but not overly verbose -- explanations.
- Do not include examples in Rust docs.

## Rules
- Always test code when done editing.
- Always write tests for new code.
- Only run tests for code that was modified.

